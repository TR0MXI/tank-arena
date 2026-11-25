use super::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io, path::Path};
use tank_arena::local_files::atomic::{self, Mode};

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Library {
    pub(super) designs: Vec<PixelSkin>,
    bindings: BTreeMap<PathBuf, usize>,
}

impl Library {
    pub(super) fn save_design(&mut self, skin: &PixelSkin) -> usize {
        self.designs
            .iter()
            .position(|s| s == skin)
            .unwrap_or_else(|| {
                self.designs.push(skin.clone());
                self.designs.len() - 1
            })
    }

    pub(super) fn bind(&mut self, key: &Path, id: usize) {
        self.bindings.insert(key.to_owned(), id);
    }

    pub(super) fn unbind(&mut self, key: &Path) {
        self.bindings.remove(key);
    }

    fn read(path: &Path) -> io::Result<Self> {
        match fs::File::open(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            file => {
                let data: Self = serde_json::from_reader(io::BufReader::new(file?))?;
                if data.bindings.values().any(|&i| i >= data.designs.len()) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid skin binding",
                    ));
                }

                Ok(data)
            }
        }
    }
}

#[derive(Resource)]
pub(in crate::game_ui) struct Looks {
    pub(super) root: PathBuf,
    pub(super) library: Library,
    pub(super) error: Option<String>,
}

impl Default for Looks {
    fn default() -> Self {
        match local_files::bots_dir() {
            Ok(root) => Self::load(root),
            Err(e) => Self {
                root: PathBuf::new(),
                library: Library::default(),
                error: Some(e.to_string()),
            },
        }
    }
}

impl Looks {
    fn load(root: PathBuf) -> Self {
        let read = Library::read(&root.with_file_name("tank-skins.json"));
        let (library, error) = match read {
            Ok(lib) => (lib, None),
            Err(e) => (Library::default(), Some(e.to_string())),
        };
        Self {
            root,
            library,
            error,
        }
    }

    pub(super) fn binding(&self, model: &Path) -> Option<usize> {
        self.library
            .bindings
            .get(model.strip_prefix(&self.root).ok()?)
            .copied()
    }

    pub(in crate::game_ui) fn resolve(
        &self,
        model: &Path,
        script: Option<&str>,
    ) -> Option<PixelSkin> {
        self.binding(model)
            .map(|id| self.library.designs[id].clone())
            .or_else(|| PixelSkin::from_script(script?))
    }

    pub(in crate::game_ui) fn preview(&self, model: &Path) -> Option<PixelSkin> {
        self.resolve(model, None).or_else(|| {
            PixelSkin::from_script(tank_arena::bot::Script::read_entry(model).ok()?.text())
        })
    }

    // 整座涂装库成功落盘后，才向运行中的游戏发布变更。
    pub(super) fn commit<T>(&mut self, edit: impl FnOnce(&mut Library) -> T) -> io::Result<T> {
        if let Some(e) = &self.error {
            return Err(io::Error::other(e.clone()));
        }

        let mut lib = self.library.clone();
        let result = edit(&mut lib);
        let path = self.root.with_file_name("tank-skins.json");
        atomic::write(&path, Mode::Replace, |out| {
            Ok(serde_json::to_writer_pretty(out, &lib)?)
        })?;
        self.library = lib;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tank_arena::appearance::{SIMPLE_HEIGHT, SIMPLE_WIDTH};

    #[test]
    fn unreadable_library_is_not_overwritten_and_failed_saves_do_not_apply() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tank-skins.json");
        fs::write(&path, b"bad").unwrap();
        let root = tmp.path().join("bots");
        let mut looks = Looks::load(root.clone());
        assert!(
            looks
                .commit(|lib| lib.save_design(&PixelSkin::template()))
                .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), b"bad");
        fs::remove_file(&path).unwrap();
        let mut looks = Looks::load(root);
        fs::create_dir(&path).unwrap();

        assert!(
            looks
                .commit(|lib| lib.save_design(&PixelSkin::template()))
                .is_err()
        );
        assert!(looks.library.designs.is_empty());
    }

    #[test]
    fn unsupported_designs_report_an_error_without_overwriting_the_library() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("bots");
        let rows = vec![".".repeat(SIMPLE_WIDTH); SIMPLE_HEIGHT]
            .join("\n")
            .replacen('.', "T", 1);
        let path = tmp.path().join("tank-skins.json");
        fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"designs": [rows], "bindings": {"a.luau": 0}}))
                .unwrap(),
        )
        .unwrap();
        let looks = Looks::load(root);
        assert!(looks.error.is_some());
        assert!(looks.library.designs.is_empty());
        assert!(path.is_file());
    }

    #[test]
    fn precedence_restart_and_identical_display_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("bots");
        let mut looks = Looks::load(root.clone());
        let a = root.join("owl.luau");
        let b = root.join("owl/bot.luau");
        let author = PixelSkin::template();
        let mut user = author.clone();
        user.set(5, 6, b'a');
        let source = format!("--[=[tank-skin\n{}\n]=]", String::from(author.clone()));
        looks
            .commit(|lib| {
                let id = lib.save_design(&user);
                lib.bind(Path::new("owl.luau"), id);
            })
            .unwrap();
        let mut looks = Looks::load(root);
        assert_eq!(looks.resolve(&a, Some(&source)), Some(user));
        assert_eq!(looks.resolve(&b, Some(&source)), Some(author.clone()));
        assert_eq!(looks.resolve(&b, None), None);
        looks.commit(|l| l.unbind(Path::new("owl.luau"))).unwrap();

        assert_eq!(looks.resolve(&a, Some(&source)), Some(author));
        assert_eq!(looks.library.designs.len(), 1);
    }
}
