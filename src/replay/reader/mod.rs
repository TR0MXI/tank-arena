mod stream;

use super::*;
use ReplayProblem as Problem;
use std::{fs::File, path::Path};
use stream::Stream;

/// 索引按固定宽度写到临时文件，长局不会让内存随关键帧数量增长。
struct SeekIndex {
    table: File,
    states: File,
}

pub struct Playback {
    pub header: Header,
    stream: Stream,
    index: SeekIndex,
    length: u64,
}

impl Playback {
    pub fn skins(&self) -> &SeatSkins {
        &self.stream.reader.skins
    }

    /// 封面只顺序读取不超过一秒的样本；不写临时索引，也不代替播放时的整局校验。
    pub fn preview(path: &Path) -> io::Result<(Header, World)> {
        let (header, mut stream) = Stream::begin(path, None)?;
        for _ in 0..header.tick_hz {
            if stream.ended {
                break;
            }
            stream.consume(None)?;
        }

        Ok((header, stream.world))
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        Self::open_while(path, || true)
    }

    /// 逐步轮询取消信号，中断时丢弃尚未发布的索引。
    pub fn open_while(path: &Path, mut keep_going: impl FnMut() -> bool) -> io::Result<Self> {
        let mut index = SeekIndex {
            table: tempfile::tempfile()?,
            states: tempfile::tempfile()?,
        };
        let (header, mut stream) = Stream::begin(path, Some(&mut index))?;
        while !stream.ended {
            if !keep_going() {
                return Err(io::ErrorKind::Interrupted.into());
            }
            stream.consume(Some(&mut index))?;
        }

        stream.check_eof()?;
        let length = stream.world.tick();
        let mut p = Self {
            header,
            stream,
            index,
            length,
        };
        p.seek(0)?;
        Ok(p)
    }

    /// 最近回放一步实际收到的控制，用于离线分析；定位到关键帧时清空。
    pub fn applied_inputs(&self) -> &[Applied] {
        &self.stream.applied
    }

    pub fn world(&self) -> &World {
        &self.stream.world
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn ended(&self) -> bool {
        self.stream.ended
    }

    pub fn advance(&mut self) -> io::Result<()> {
        if !self.stream.ended {
            self.stream.consume(None)?;
        }
        if self.world().tick() == self.length {
            while !self.stream.ended {
                self.stream.consume(None)?;
            }
        }
        Ok(())
    }

    pub fn seek(&mut self, tick: u64) -> io::Result<()> {
        let target = tick.min(self.length);
        self.stream.restore(&mut self.index, target)?;
        while self.world().tick() < target || target == self.length && !self.stream.ended {
            self.stream.consume(None)?;
        }
        Ok(())
    }
}
