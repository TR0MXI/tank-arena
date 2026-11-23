//! 两层独立边界：每个 bitcode 批次最多 32 条，多个完整批次再合成一个 zstd 块。

mod decoder;
mod encoder;
#[cfg(test)]
mod tests;

use super::{Record, invalid};
use crate::sim::ItemPool;
pub(super) use decoder::Decoder;
pub(super) use encoder::Encoder;
use std::io;
const ITEM_MAGIC: &[u8; 8] = b"TANKREC\x06";

const MAGIC: &[u8; 8] = b"TANKREC\x05";
const TARGET: usize = 32 * 1024;
const MAX_BLOCK: usize = 256 * 1024;
const LENGTH: usize = size_of::<u32>();
const CAPACITY: usize = 32;

// 固定槽数让解码器在分配外层记录前就知道上界，不信任文件中声明的 Vec 长度。
#[derive(Default, bitcode::Encode, bitcode::Decode)]
struct Batch([Option<Record>; CAPACITY]);

impl Batch {
    fn unpack_into(self, pending: &mut Vec<Record>) -> io::Result<()> {
        let count = self.0.iter().take_while(|r| r.is_some()).count();
        if count == 0 || self.0[count..].iter().any(Option::is_some) {
            return Err(invalid("empty or non-contiguous replay batch"));
        }
        if count > 1
            && self.0[..count]
                .iter()
                .any(|r| !matches!(r, Some(Record::Step { .. })))
        {
            return Err(invalid("non-step record must occupy its own batch"));
        }

        // 倒序暂存，next() 只移动一条记录，不复制动作和事件向量。
        pending.extend(self.0.into_iter().take(count).rev().flatten());
        Ok(())
    }
}
