//! 从 Finder 启动时，可写内容放在 .app 旁；从命令行启动时放在工作目录。

pub mod atomic;
mod bootstrap;
pub use bootstrap::prepare;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy)]
pub enum Folder {
    Maps,
    Replays,
}

fn app_bundle(exe: &Path) -> Option<&Path> {
    exe.ancestors().nth(3).filter(|p| {
        p.extension().is_some_and(|x| x == "app")
            && exe.parent().is_some_and(|p| p.ends_with("Contents/MacOS"))
    })
}

fn content_dir(exe: &Path, cwd: &Path, name: &str) -> PathBuf {
    app_bundle(exe)
        .and_then(Path::parent)
        .unwrap_or(cwd)
        .join(name)
}

fn writable_dir(name: &str) -> io::Result<PathBuf> {
    Ok(content_dir(
        &std::env::current_exe()?,
        &std::env::current_dir()?,
        name,
    ))
}

pub fn bots_dir() -> io::Result<PathBuf> {
    writable_dir("bots")
}

impl Folder {
    pub fn path(self) -> io::Result<PathBuf> {
        writable_dir(match self {
            Self::Maps => "maps",
            Self::Replays => "demo",
        })
    }

    pub fn scan(self, dir: &Path) -> io::Result<Vec<Entry>> {
        let ext = match self {
            Self::Maps => "map",
            Self::Replays => crate::replay::EXTENSION,
        };
        let items = match fs::read_dir(dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
            v => v?,
        };
        let mut entries = Vec::new();
        for item in items {
            let item = item?;
            let path = item.path();
            if !path
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case(ext))
            {
                continue;
            }

            let meta = match item.metadata() {
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                v => v?,
            };
            if meta.is_file() {
                entries.push(Entry::from_metadata(path, &meta)?);
            }
        }

        entries.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Entry {
    pub path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

impl Entry {
    fn from_metadata(path: PathBuf, meta: &fs::Metadata) -> io::Result<Self> {
        Ok(Self {
            path,
            bytes: meta.len(),
            modified: meta.modified()?,
            // 原子替换可能保留文件大小和修改时间。
            #[cfg(unix)]
            identity: (meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec()),
        })
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }

        Self::from_metadata(path.to_owned(), &meta)
    }

    /// 对当前路径重新取版本，不使用界面上一次轮询的结果。
    pub fn is_current(&self) -> bool {
        Self::read(&self.path).as_ref().ok() == Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_directories() {
        let exe = Path::new("/Games/Tank Arena.app/Contents/MacOS/tank-arena");
        let cwd = Path::new("/work");
        for name in ["maps", "demo", "bots"] {
            assert_eq!(content_dir(exe, cwd, name), Path::new("/Games").join(name));
            assert_eq!(
                content_dir(Path::new("/bin/arena"), cwd, name),
                cwd.join(name)
            );
        }
    }

    #[test]
    fn detects_add_replace_remove_and_ignores_sidecars() {
        let dir = tempfile::tempdir().unwrap();
        let scan = || Folder::Replays.scan(dir.path()).unwrap();
        let file = dir.path().join("test.TANKREC");
        fs::write(dir.path().join("test.result.json"), "{}").unwrap();

        assert!(scan().is_empty());
        fs::write(&file, "1").unwrap();
        let old = scan();
        assert_eq!(old.len(), 1);
        fs::write(&file, "2 bytes").unwrap();

        assert_ne!(old, scan());
        #[cfg(unix)]
        {
            let old = scan();
            let replacement = dir.path().join("copy.tmp");
            fs::write(&replacement, "7 bytes").unwrap();
            fs::File::open(&replacement)
                .unwrap()
                .set_modified(old[0].modified)
                .unwrap();
            fs::rename(replacement, &file).unwrap();
            assert_ne!(old, scan());
        }
        fs::remove_file(file).unwrap();
        assert!(scan().is_empty());
    }
}
