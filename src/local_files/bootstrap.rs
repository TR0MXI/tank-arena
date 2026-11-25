use super::{Folder, bots_dir};
use std::{fs, io, path::Path};

const BOTS: &[(&str, &[u8])] = &[(
    "example.luau",
    include_bytes!("../../assets/bots/example.luau"),
)];
const MAPS: &[(&str, &[u8])] = &[
    ("switchyard.map", crate::sim::SWITCHYARD.as_bytes()),
    (
        "bank-shot.map",
        include_bytes!("../../assets/maps/bank-shot.map"),
    ),
];

// 仅新建成功的一方放入示例；已有目录即使为空也属于用户。
fn seed(dir: &Path, files: &[(&str, &[u8])]) -> io::Result<()> {
    match fs::create_dir(dir) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => return Ok(()),
        Err(e) => return Err(e),
    }
    for (name, contents) in files {
        fs::write(dir.join(name), contents)?;
    }
    Ok(())
}

/// 准备便携内容目录，不覆盖用户改过的初始文件，也不补回已删除的文件。
pub fn prepare() -> io::Result<()> {
    seed(&bots_dir()?, BOTS)?;
    fs::create_dir_all(Folder::Replays.path()?)?;
    seed(&Folder::Maps.path()?, MAPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_map_library_is_seeded_once_from_bundled_maps() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("maps");
        let other = tmp.path().join("map");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("custom.map"), "custom").unwrap();
        let templates = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/maps");
        seed(&dir, MAPS).unwrap();
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
        seed(&dir, MAPS).unwrap();
        assert_eq!(fs::read_to_string(edited).unwrap(), "edited");
        assert!(!removed.exists());
        assert_eq!(
            fs::read_to_string(other.join("custom.map")).unwrap(),
            "custom"
        );
    }

    #[test]
    fn existing_bots_are_never_overwritten_or_reinstalled() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("bots");
        seed(&dir, BOTS).unwrap();

        let entry = dir.join("example.luau");
        assert_eq!(
            crate::bot::catalog::discover(&dir).unwrap(),
            std::slice::from_ref(&entry)
        );
        assert_eq!(
            fs::read(&entry).unwrap(),
            include_bytes!("../../assets/bots/example.luau").as_slice()
        );
        fs::write(&entry, "my script").unwrap();
        seed(&dir, BOTS).unwrap();

        assert_eq!(fs::read_to_string(&entry).unwrap(), "my script");

        fs::remove_file(&entry).unwrap();
        seed(&dir, BOTS).unwrap();
        assert!(!entry.exists());
    }
}
