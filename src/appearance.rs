//! 仅保存外观数据；读取脚本声明的像素时无需执行 Luau 或加载外部资源。

use serde::{Deserialize, Serialize};

pub const SIMPLE_WIDTH: usize = 24;
pub const SIMPLE_HEIGHT: usize = 16;
pub const SCALE: usize = 2;
pub const WIDTH: usize = SIMPLE_WIDTH * SCALE;
pub const HEIGHT: usize = SIMPLE_HEIGHT * SCALE;
/// 画布的十六分之一，相当于简易编辑器中的 24 个整格。
pub const MIN_TEAM_PIXELS: usize = WIDTH * HEIGHT / 16;
pub const PALETTE: &[u8; 13] = b".XsgwTaNBPGEM";
pub const TEMPLATE: &str = "\
........................
....XXXXXXXXXXXXXXXX....
...XXsgsgsgsgsgsgsgXX...
...XXssssssssssssssXX...
....XggTTTTTTTTTTggX....
....XsgTTXXXXXXTTgsX....
....XsgTXTTwTTTXggXX....
....XssTXTTTaTTXXXXsssX.
....XssTXTTTTTTXXXXXXXX.
....XsgTXTTTTTTXggXX....
....XsgTTXXXXXXTTgsX....
....XggTTTTTTTTTTggX....
...XXssssssssssssssXX...
...XXsgsgsgsgsgsgsgXX...
....XXXXXXXXXXXXXXXX....
........................";

/// 按可读、可校验的像素行序列化，不使用图片路径或可执行代码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PixelSkin([u8; WIDTH * HEIGHT]);

impl TryFrom<String> for PixelSkin {
    type Error = &'static str;

    fn try_from(rows: String) -> Result<Self, Self::Error> {
        if rows.lines().next().map(str::len) != Some(WIDTH) {
            return Err("expected a 48×32 saved design");
        }
        Self::parse(&rows)
    }
}

impl From<PixelSkin> for String {
    fn from(s: PixelSkin) -> Self {
        s.0.as_chunks::<WIDTH>()
            .0
            .iter()
            .map(|r| std::str::from_utf8(r).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl PixelSkin {
    pub fn parse(rows: &str) -> Result<Self, &'static str> {
        Self::read_rows(rows)?.require_team_color()
    }

    fn read_rows(rows: &str) -> Result<Self, &'static str> {
        fn copy_rows(rows: &str, width: usize, out: &mut [u8]) -> Result<(), &'static str> {
            let mut lines = rows.lines();
            for dst in out.chunks_exact_mut(width) {
                let src = lines.next().ok_or("missing skin rows")?.as_bytes();
                if src.len() != width {
                    return Err("inconsistent skin width");
                }
                dst.copy_from_slice(src);
            }

            if lines.next().is_some() {
                return Err("too many skin rows");
            }

            Ok(())
        }
        match rows.lines().next().map(str::len) {
            Some(SIMPLE_WIDTH) => {
                let mut src = [0; SIMPLE_WIDTH * SIMPLE_HEIGHT];
                copy_rows(rows, SIMPLE_WIDTH, &mut src)?;
                Self::from_simple_cells(&src)
            }
            Some(WIDTH) => {
                let mut src = [0; WIDTH * HEIGHT];
                copy_rows(rows, WIDTH, &mut src)?;
                Self::read_cells(src)
            }
            _ => Err("expected a 24×16 or 48×32 skin"),
        }
    }

    fn read_cells(data: [u8; WIDTH * HEIGHT]) -> Result<Self, &'static str> {
        if !data.contains(&b'T') || !data.iter().all(|p| PALETTE.contains(p)) {
            return Err("invalid skin palette or missing team color");
        }
        Ok(Self(data))
    }

    fn require_team_color(self) -> Result<Self, &'static str> {
        self.valid()
            .then_some(self)
            .ok_or("insufficient team color")
    }

    pub(crate) fn from_cells(data: [u8; WIDTH * HEIGHT]) -> Result<Self, &'static str> {
        Self::read_cells(data)?.require_team_color()
    }

    /// 简笔绘图模式的每个像素对应 2×2 个细格。
    fn from_simple_cells(src: &[u8; SIMPLE_WIDTH * SIMPLE_HEIGHT]) -> Result<Self, &'static str> {
        let mut out = [0; WIDTH * HEIGHT];
        for (y, row) in out.as_chunks_mut::<WIDTH>().0.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = src[(y / SCALE) * SIMPLE_WIDTH + x / SCALE];
            }
        }

        Self::read_cells(out)
    }

    pub fn template() -> Self {
        Self::parse(TEMPLATE).expect("bundled pixel template")
    }

    pub fn cells(&self) -> &[u8; WIDTH * HEIGHT] {
        &self.0
    }

    pub fn set(&mut self, x: usize, y: usize, brush: u8) {
        if x < WIDTH && y < HEIGHT && PALETTE.contains(&brush) {
            self.0[y * WIDTH + x] = brush;
        }
    }

    pub fn team_pixels(&self) -> usize {
        self.0.iter().filter(|&&v| v == b'T').count()
    }

    pub fn valid(&self) -> bool {
        self.team_pixels() >= MIN_TEAM_PIXELS
    }

    pub fn from_script(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        lines.find(|l| l.trim() == "--[=[tank-skin")?;
        let mut rows = String::new();
        for row in lines.take(HEIGHT + 1) {
            if row.trim() == "]=]" {
                return Self::parse(&rows).ok();
            }
            if !matches!(row.len(), SIMPLE_WIDTH | WIDTH) {
                return None;
            }

            rows.push_str(row);
            rows.push('\n');
        }

        None
    }
}

pub fn rgba(p: u8, team: [u8; 3]) -> [u8; 4] {
    let [r, g, b] = match p {
        b'X' => [38, 52, 55],
        b's' => [104, 126, 124],
        b'g' => [161, 179, 154],
        b'w' => [241, 236, 201],
        b'T' => team,
        b'a' => [227, 168, 78],
        b'N' => [26, 40, 67],
        b'B' => [57, 73, 166],
        b'P' => [203, 208, 226],
        b'G' => [248, 215, 151],
        b'E' => [141, 183, 115],
        b'M' => [65, 85, 113],
        _ => return [0; 4],
    };
    [r, g, b, 255]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_rows_expand_losslessly_and_fine_author_rows_retain_each_column() {
        let simple = PixelSkin::parse(TEMPLATE).unwrap();
        assert_eq!((WIDTH, HEIGHT), (48, 32));
        for (y, row) in TEMPLATE.lines().enumerate() {
            for (x, v) in row.bytes().enumerate() {
                for dy in 0..2 {
                    let i = (2 * y + dy) * WIDTH + 2 * x;
                    assert_eq!(simple.cells()[i..i + 2], [v; 2]);
                }
            }
        }

        let mut fine = simple;
        fine.set(47, 31, b'a');
        let rows = String::from(fine.clone());
        assert_eq!(rows.lines().count(), HEIGHT);
        assert!(
            PixelSkin::parse(
                &rows
                    .lines()
                    .take(SIMPLE_HEIGHT)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
            .is_err()
        );
        assert!(rows.lines().all(|r| r.len() == WIDTH));
        let block = format!("--[=[tank-skin\n{rows}\n]=]");
        assert_eq!(PixelSkin::from_script(&block), Some(fine));
        assert!(
            PixelSkin::parse(&rows.replacen(&".".repeat(WIDTH), &".".repeat(SIMPLE_WIDTH), 1))
                .is_err()
        );
    }

    #[test]
    fn team_minimum_is_identical_in_both_resolutions() {
        assert_eq!(MIN_TEAM_PIXELS, 96);
        for (width, height, min) in [(SIMPLE_WIDTH, SIMPLE_HEIGHT, 24), (WIDTH, HEIGHT, 96)] {
            for (n, ok) in [(min - 1, false), (min, true)] {
                let mut data = vec![b'.'; width * height];
                data[..n].fill(b'T');
                let rows = data
                    .chunks(width)
                    .map(|r| std::str::from_utf8(r).unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                assert_eq!(PixelSkin::parse(&rows).is_ok(), ok);
                assert_eq!(
                    PixelSkin::from_script(&format!("--[=[tank-skin\n{rows}\n]=]")).is_some(),
                    ok
                );
            }
        }
    }

    #[test]
    fn author_blocks_are_inert_bounded_and_optional() {
        let block = format!("--!strict\n--[=[tank-skin\n{TEMPLATE}\n]=]\nreturn decide");
        assert_eq!(
            PixelSkin::from_script(&block.replace('\n', "\r\n")),
            Some(PixelSkin::template())
        );
        for bad in [
            block.replace("w", "?"),
            block.replace("T", "s"),
            block.replace("]=]", ""),
            block.replace("w", "ws"),
            "return decide".into(),
        ] {
            assert!(PixelSkin::from_script(&bad).is_none());
        }
    }
}
