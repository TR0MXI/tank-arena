use crate::local_files::atomic::{self, Mode};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// 只发布已在暂存区完整写好的回放；即使磁盘已满或名称冲突，
/// 也不能让目录监视器看到未写完的文件。
pub fn save_staged(source: &Path, dir: &Path) -> io::Result<PathBuf> {
    let name = source.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let dest = dir.join(name);
    atomic::write(&dest, Mode::CreateNew, |out| {
        io::copy(&mut fs::File::open(source)?, out)?;
        Ok(())
    })?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishing_never_overwrites_a_file_dropped_by_the_player() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("demo");
        let staged = tmp.path().join("a.tankrec");
        fs::write(&staged, "first").unwrap();
        let dest = save_staged(&staged, &dir).unwrap();
        fs::write(&staged, "different").unwrap();

        assert_eq!(
            save_staged(&staged, &dir).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(dest).unwrap(), b"first");
        assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
    }
}
