/// 固定 32 桶的微秒直方图；返回对应分位数的微秒上界，不存储整局历史。
#[derive(Default)]
pub(super) struct Histogram {
    bins: [u64; 32],
    count: u64,
}

impl Histogram {
    pub fn add(&mut self, v: u64) {
        let bucket = u64::BITS - v.leading_zeros();
        self.bins[bucket.min(31) as usize] += 1;
        self.count += 1;
    }

    pub fn upper(&self, percent: u64) -> u64 {
        let mut n = (self.count as u128 * u128::from(percent)).div_ceil(100);
        for (i, &v) in self.bins.iter().enumerate() {
            if n <= u128::from(v) {
                return if i == 31 { u64::MAX } else { (1 << i) - 1 };
            }
            n -= u128::from(v);
        }

        0
    }
}
