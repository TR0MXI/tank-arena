use super::{ArenaMap, MapError};

impl ArenaMap {
    pub const GENERATION_AXIS: std::ops::RangeInclusive<usize> = 4..=32;

    /// 用随机生成树保证地图连通，再打开额外通道供绕行。
    /// 直接操作交换格式，让生成、保存、回放和物理模拟读取同一布局。
    pub fn generate(seed: u64, [w, h]: [usize; 2]) -> Result<Self, MapError> {
        if !Self::GENERATION_AXIS.contains(&w) || !Self::GENERATION_AXIS.contains(&h) {
            return Err(MapError::GenerationSize);
        }

        let mut rng = fastrand::Rng::with_seed(seed);
        let mut rows = vec![vec![b'.'; 2 * w + 1]; 2 * h + 1];
        for (y, row) in rows.iter_mut().enumerate() {
            for (x, b) in row.iter_mut().enumerate() {
                *b = match (x % 2, y % 2) {
                    (0, 0) => b'+',
                    (1, 0) => b'-',
                    (0, 1) => b'|',
                    _ => b'.',
                };
            }
        }

        let mut visited = vec![vec![false; w]; h];
        let mut stack = vec![(0usize, 0usize)];
        visited[0][0] = true;
        while let Some(&(x, y)) = stack.last() {
            let mut dirs = [(0, 1), (0, -1), (1, 0), (-1, 0)];
            rng.shuffle(&mut dirs);
            if let Some((u, v)) = dirs
                .into_iter()
                .map(|(dx, dy)| (x.wrapping_add_signed(dx), y.wrapping_add_signed(dy)))
                .find(|&(u, v)| u < w && v < h && !visited[v][u])
            {
                rows[y + v + 1][x + u + 1] = b' ';
                visited[v][u] = true;
                stack.push((u, v));
            } else {
                stack.pop();
            }
        }

        for row in &mut rows[1..2 * h] {
            for edge in &mut row[1..2 * w] {
                if matches!(*edge, b'-' | b'|') && rng.usize(..3) == 0 {
                    *edge = b' ';
                }
            }
        }

        // 每个水平分区放一对出生点；即使只有两辆坦克也不会挤在一起。
        for band in 0..4 {
            let y = 2 * rng.usize(band * h / 4..(band + 1) * h / 4) + 1;
            rows[y][2 * rng.usize(..w / 3) + 1] = b'>';
            rows[y][2 * rng.usize(2 * w / 3..w) + 1] = b'<';
        }

        rows.iter()
            .map(|row| std::str::from_utf8(row).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            .parse()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_fields_round_trip_and_fit_a_full_roster() {
        for seed in 0..48 {
            for size in [[4, 4], [4, 32], [32, 4], [17, 9], [32, 32]] {
                let a = ArenaMap::generate(seed, size).unwrap();
                assert_eq!(a.size(), (size[0], size[1]));
                assert_eq!(a.spawns().len(), 8);
                let b: ArenaMap = a.source().parse().unwrap();
                assert_eq!(a.spawns(), b.spawns());
                assert!(a.wall_rectangles().eq(b.wall_rectangles()));
            }
        }

        let gen_map = |seed| ArenaMap::generate(seed, [12, 8]).unwrap();
        assert_eq!(gen_map(1).source(), gen_map(1).source());
        assert_ne!(gen_map(1).source(), gen_map(2).source());
        for size in [[0, 8], [12, 3], [4, usize::MAX]] {
            assert!(ArenaMap::generate(1, size).is_err());
        }
    }
}
