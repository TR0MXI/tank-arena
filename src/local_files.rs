//! 从 Finder 启动时，可写内容放在 .app 旁；从命令行启动时放在工作目录。

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

// 只初始化新建的目录；用户可以选择保留已有的空目录。
fn prepare_bots(dir: &Path, defaults: &Path) -> io::Result<()> {
    match fs::create_dir(dir) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => return Ok(()),
        Err(e) => return Err(e),
    }

    // 发布包和开发目录都只提供 API 示例；不要把开发者自建的模型复制到新目录。
    let entry = "example.luau";
    fs::copy(defaults.join(entry), dir.join(entry))?;
    Ok(())
}

fn prepare_maps(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }

    // 只在新目录不存在时接手旧地图库，不合并或覆盖两边的同名地图。
    let legacy = dir.with_file_name("map");
    if !dir.try_exists()? && legacy.is_dir() {
        return fs::rename(legacy, dir);
    }

    fs::create_dir(dir)?;
    for (name, source) in [
        ("switchyard.map", crate::sim::SWITCHYARD),
        (
            "bank-shot.map",
            include_str!("../assets/maps/bank-shot.map"),
        ),
    ] {
        fs::write(dir.join(name), source)?;
    }
    Ok(())
}

/// 准备便携内容目录，不覆盖用户改过的初始文件，也不补回已删除的文件。
pub fn prepare() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let defaults = app_bundle(&exe).map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/bots"),
        |app| app.join("Contents/Resources/bots"),
    );
    prepare_bots(&bots_dir()?, &defaults)?;
    fs::create_dir_all(Folder::Replays.path()?)?;
    prepare_maps(&Folder::Maps.path()?)
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
    fn new_map_library_is_seeded_once_from_bundled_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("maps");
        let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps");
        prepare_maps(&dir).unwrap();
        let entries = Folder::Maps.scan(&dir).unwrap();
        assert_eq!(entries.len(), Folder::Maps.scan(&templates).unwrap().len());
        for entry in entries {
            let original = templates.join(entry.path.file_name().unwrap());
            assert_eq!(fs::read(original).unwrap(), fs::read(entry.path).unwrap());
        }

        let edited = dir.join("switchyard.map");
        let removed = dir.join("bank-shot.map");
        fs::write(&edited, "edited").unwrap();
        fs::remove_file(&removed).unwrap();
        prepare_maps(&dir).unwrap();
        assert_eq!(fs::read_to_string(edited).unwrap(), "edited");
        assert!(!removed.exists());
        assert!(!tmp.path().join("map").exists());
    }

    #[test]
    fn existing_library_migrates_only_if_new_directory_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("maps");
        let legacy = tmp.path().join("map");
        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("custom.map"), "custom").unwrap();
        prepare_maps(&dir).unwrap();
        assert!(!legacy.exists());
        assert_eq!(Folder::Maps.scan(&dir).unwrap().len(), 1);
        assert_eq!(
            fs::read_to_string(dir.join("custom.map")).unwrap(),
            "custom"
        );

        fs::create_dir(&legacy).unwrap();
        fs::write(legacy.join("custom.map"), "other").unwrap();
        prepare_maps(&dir).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("custom.map")).unwrap(),
            "custom"
        );
        assert_eq!(
            fs::read_to_string(legacy.join("custom.map")).unwrap(),
            "other"
        );
    }

    #[test]
    fn existing_bots_are_never_overwritten_or_reinstalled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bots");
        let defaults = tmp.path().join("Resources/bots");
        fs::create_dir_all(&defaults).unwrap();
        fs::write(defaults.join("example.luau"), "example").unwrap();
        fs::write(defaults.join("private.luau"), "private").unwrap();
        prepare_bots(&dir, &defaults).unwrap();

        let entry = dir.join("example.luau");
        assert_eq!(
            crate::catalog::discover(&dir).unwrap(),
            std::slice::from_ref(&entry)
        );
        fs::write(&entry, "my script").unwrap();
        prepare_bots(&dir, &defaults).unwrap();

        assert_eq!(fs::read_to_string(&entry).unwrap(), "my script");

        fs::remove_file(&entry).unwrap();
        prepare_bots(&dir, &defaults).unwrap();
        assert!(!entry.exists());
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
