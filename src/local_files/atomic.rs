use std::{
    fs,
    io::{self, BufWriter, Write},
    path::Path,
};

/// 同目录暂存成功后才发布；已有目标是否可被覆盖由调用方显式指定。
#[derive(Clone, Copy)]
pub enum Mode {
    CreateNew,
    Replace,
}

pub fn write(
    path: &Path,
    mode: Mode,
    fill: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing output directory"))?;
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".arena-")
        .suffix(".part")
        .tempfile_in(dir)?;
    {
        let mut buffer = BufWriter::new(tmp.as_file_mut());
        fill(&mut buffer)?;
        buffer.flush()?;
    }
    tmp.as_file().sync_all()?;
    match mode {
        Mode::CreateNew => tmp.persist_noclobber(path)?,
        Mode::Replace => tmp.persist(path)?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_write_never_publishes_partial_content() {
        for mode in [Mode::CreateNew, Mode::Replace] {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("data");
            let original = if matches!(mode, Mode::Replace) {
                fs::write(&path, "original").unwrap();
                Some(String::from("original"))
            } else {
                None
            };
            let result = write(&path, mode, |file| {
                file.write_all(b"partial")?;
                file.flush()?;
                Err(io::ErrorKind::StorageFull.into())
            });
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::StorageFull);
            assert_eq!(fs::read_to_string(path).ok(), original);
            assert_eq!(
                fs::read_dir(tmp.path()).unwrap().count(),
                usize::from(original.is_some())
            );
        }
    }
}
