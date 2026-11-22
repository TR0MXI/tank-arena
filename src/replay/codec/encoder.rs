use super::*;
use crate::runner::Decision;
use std::{io::Write, mem::size_of_val};

pub(in crate::replay) struct Encoder<W> {
    codec: bitcode::Buffer,
    batch: Batch,
    count: usize,
    staged_bytes: usize,
    block: BlockWriter<W>,
}

struct BlockWriter<W> {
    output: W,
    raw: Vec<u8>,
    compressor: zstd::bulk::Compressor<'static>,
}

impl<W: Write> Encoder<W> {
    #[cfg(test)]
    pub(in crate::replay) fn new(output: W) -> io::Result<Self> {
        Self::with_skins(output, &Default::default())
    }

    pub(in crate::replay) fn with_skins(
        mut output: W,
        skins: &crate::replay::SeatSkins,
    ) -> io::Result<Self> {
        let mask = skins
            .iter()
            .enumerate()
            .fold(0u8, |m, (i, s)| m | (u8::from(s.is_some()) << i));
        output.write_all(MAGIC)?;
        output.write_all(&[mask])?;
        for skin in skins.iter().flatten() {
            if !skin.valid() {
                return Err(invalid("insufficient team color"));
            }
            output.write_all(skin.cells())?;
        }

        Ok(Self {
            codec: bitcode::Buffer::new(),
            batch: Batch::default(),
            count: 0,
            staged_bytes: 0,
            block: BlockWriter {
                output,
                raw: Vec::with_capacity(TARGET),
                compressor: zstd::bulk::Compressor::new(3)?,
            },
        })
    }

    pub(in crate::replay) fn push(&mut self, record: Record) -> io::Result<()> {
        let step_bytes = staging_cost(&record);
        // 元数据和关键帧单独成批；索引从关键帧之后恢复，不需要重新解前一批动作。
        if step_bytes.is_none_or(|n| self.staged_bytes.saturating_add(n) > TARGET) {
            self.finish_batch()?;
        }
        self.batch.0[self.count] = Some(record);
        self.count += 1;
        self.staged_bytes += step_bytes.unwrap_or(0);
        if step_bytes.is_none() || self.count == CAPACITY || self.staged_bytes >= TARGET {
            self.finish_batch()?;
        }
        Ok(())
    }

    fn finish_batch(&mut self) -> io::Result<()> {
        if self.count != 0 {
            self.block.push(self.codec.encode(&self.batch))?;
            self.batch.0[..self.count].fill_with(|| None);
            self.count = 0;
            self.staged_bytes = 0;
        }
        Ok(())
    }

    pub(in crate::replay) fn flush(&mut self) -> io::Result<()> {
        self.finish_batch()?;
        self.block.flush()
    }
}

// 写线程接管队列中的所有权。按动态字段估算暂存预算，不为测长度而先逐条序列化一次。
fn staging_cost(record: &Record) -> Option<usize> {
    let Record::Step { inputs, events, .. } = record else {
        return None;
    };

    let messages = inputs
        .iter()
        .map(|a| match &a.decision {
            Decision::Received {
                result: Err(fault), ..
            } => fault.message.len(),
            _ => 0,
        })
        .sum::<usize>();
    Some(
        size_of_val(record)
            + size_of_val(inputs.as_slice())
            + size_of_val(events.as_slice())
            + messages,
    )
}

impl<W: Write> BlockWriter<W> {
    fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > MAX_BLOCK - LENGTH {
            return Err(invalid("replay batch too large"));
        }

        if self.raw.len() + LENGTH + bytes.len() > TARGET {
            self.flush()?;
        }
        self.raw
            .extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        self.raw.extend_from_slice(bytes);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.raw.is_empty() {
            return Ok(());
        }

        let zipped = self.compressor.compress(&self.raw)?;
        self.output
            .write_all(&(self.raw.len() as u32).to_le_bytes())?;
        self.output
            .write_all(&(zipped.len() as u32).to_le_bytes())?;
        self.output.write_all(&zipped)?;
        self.raw.clear();
        Ok(())
    }
}
