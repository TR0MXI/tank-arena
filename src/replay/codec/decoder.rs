use super::*;
use crate::{
    appearance::{HEIGHT, PixelSkin, WIDTH},
    replay::{ReplayIssue, ReplayProblem, SeatSkins},
};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
};

pub(in crate::replay) struct Decoder {
    pub(in crate::replay) skins: SeatSkins,
    pub(in crate::replay) pool: ItemPool,
    codec: bitcode::Buffer,
    pending: Vec<Record>,
    file: BufReader<File>,
    plain: Vec<u8>,
    zipped: Vec<u8>,
    start: u64,
    offset: usize,
    decompressor: zstd::bulk::Decompressor<'static>,
}

impl Decoder {
    pub(in crate::replay) fn new(file: File) -> io::Result<Self> {
        let mut file = BufReader::new(file);
        let mut magic = [0; 8];
        file.read_exact(&mut magic)?;
        if magic[..7] != MAGIC[..7] {
            return Err(invalid("not a current Tank Arena recording"));
        }
        if magic != *MAGIC && magic != *ITEM_MAGIC {
            return Err(ReplayIssue::at(
                1,
                ReplayProblem::Version,
                "unsupported binary revision",
            ));
        }

        let pool = if magic == *ITEM_MAGIC {
            let mut mask = [0; 2];
            file.read_exact(&mut mask)?;
            ItemPool::from_bits(u16::from_le_bytes(mask))
                .ok_or_else(|| invalid("unknown crate types"))?
        } else {
            ItemPool::NONE
        };
        let mut skins: SeatSkins = Default::default();
        let mut mask = [0];
        file.read_exact(&mut mask)?;
        for (i, slot) in skins.iter_mut().enumerate() {
            if mask[0] & (1 << i) != 0 {
                let mut cells = [0; WIDTH * HEIGHT];
                file.read_exact(&mut cells)?;
                *slot = Some(PixelSkin::from_cells(cells).map_err(invalid)?);
            }
        }

        Ok(Self {
            skins,
            pool,
            start: file.stream_position()?,
            file,
            codec: bitcode::Buffer::new(),
            pending: Vec::with_capacity(CAPACITY),
            plain: vec![],
            zipped: vec![],
            offset: 0,
            decompressor: zstd::bulk::Decompressor::new()?,
        })
    }

    fn load(&mut self) -> io::Result<bool> {
        if self.file.fill_buf()?.is_empty() {
            return Ok(false);
        }

        self.start = self.file.stream_position()?;
        let mut lengths = [0; 8];
        self.file.read_exact(&mut lengths)?;
        let raw = u32::from_le_bytes(lengths[..4].try_into().unwrap()) as usize;
        let zipped = u32::from_le_bytes(lengths[4..].try_into().unwrap()) as usize;
        if !(1..=MAX_BLOCK).contains(&raw)
            || !(1..=zstd::zstd_safe::compress_bound(raw)).contains(&zipped)
        {
            return Err(invalid("invalid replay block length"));
        }

        self.zipped.resize(zipped, 0);
        self.file.read_exact(&mut self.zipped)?;
        self.plain.resize(raw, 0);
        if self
            .decompressor
            .decompress_to_buffer(&self.zipped, self.plain.as_mut_slice())?
            != raw
        {
            return Err(invalid("invalid decompressed length"));
        }

        self.offset = 0;
        Ok(true)
    }

    pub(in crate::replay) fn next(&mut self) -> io::Result<Option<Record>> {
        if let Some(record) = self.pending.pop() {
            return Ok(Some(record));
        }
        if self.offset == self.plain.len() && !self.load()? {
            return Ok(None);
        }

        let mut tail = &self.plain[self.offset..];
        let prefix = tail
            .split_off(..LENGTH)
            .ok_or_else(|| invalid("missing batch length"))?;
        let len = u32::from_le_bytes(prefix.try_into().unwrap()) as usize;
        let payload = tail
            .get(..len)
            .ok_or_else(|| invalid("batch exceeds block"))?;
        let batch: Batch = self.codec.decode(payload).map_err(invalid)?;
        batch.unpack_into(&mut self.pending)?;
        self.offset += LENGTH + len;
        Ok(self.pending.pop())
    }

    pub(in crate::replay) fn position(&self) -> (u64, u64) {
        debug_assert!(
            self.pending.is_empty(),
            "index only after singleton keyframe"
        );
        (self.start, self.offset as u64)
    }

    pub(in crate::replay) fn restore(&mut self, start: u64, offset: u64) -> io::Result<()> {
        // 即使跳到同一解压块，也不能继续派发上一次定位留下的记录。
        self.pending.clear();
        if start != self.start {
            self.file.seek(SeekFrom::Start(start))?;
            if !self.load()? {
                return Err(invalid("missing indexed block"));
            }
        }
        self.offset = usize::try_from(offset)
            .ok()
            .filter(|&i| i <= self.plain.len())
            .ok_or_else(|| invalid("invalid block cursor"))?;
        Ok(())
    }
}
