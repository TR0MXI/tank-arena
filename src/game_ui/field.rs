use super::*;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Read},
    path::Path,
    sync::Arc,
};
use tank_arena::{
    local_files::{
        Entry,
        atomic::{self, Mode},
    },
    sim::MAP_LIMIT,
};

pub(super) struct CachedMap {
    pub entry: Entry,
    // 外部编辑器写入期间，部署面板继续使用最近一次有效的地图。
    map: Option<Arc<ArenaMap>>,
    readable: bool,
}

impl CachedMap {
    pub fn on_disk(&self) -> Option<&ArenaMap> {
        self.map.as_deref().filter(|_| self.readable)
    }
}

#[derive(Resource)]
pub(super) struct Field {
    pub(super) map: Arc<ArenaMap>,
    files: Vec<CachedMap>,
    selected: Option<PathBuf>,
    generated: Option<Arc<ArenaMap>>,
    pub(super) scan_error: Option<String>,
}

impl Default for Field {
    fn default() -> Self {
        Self {
            // 没有可用文件时提供可保存的草稿，而不是继续使用已删除的内置地图。
            map: Arc::new(ArenaMap::generate(0, [12, 8]).expect("valid fallback size")),
            files: vec![],
            selected: None,
            generated: None,
            scan_error: None,
        }
    }
}

fn load(path: &Path) -> io::Result<ArenaMap> {
    let mut text = String::new();
    File::open(path)?
        .take(MAP_LIMIT as u64 + 1)
        .read_to_string(&mut text)?;
    text.parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

impl Field {
    /// 只重新解析有变化的文件；外部写入尚未完成时，保留已知有效的地图。
    fn reconcile(&mut self, entries: Vec<Entry>) -> bool {
        if self.files.iter().map(|f| &f.entry).eq(&entries) {
            return false;
        }

        let mut old: BTreeMap<_, _> = std::mem::take(&mut self.files)
            .into_iter()
            .map(|f| (f.entry.path.clone(), f))
            .collect();
        self.files = entries
            .into_iter()
            .map(|entry| {
                let previous_map = match old.remove(&entry.path) {
                    Some(cached) if cached.entry == entry => return cached,
                    cached => cached.and_then(|file| file.map),
                };

                let parsed = load(&entry.path).ok().map(Arc::new);
                CachedMap {
                    entry,
                    readable: parsed.is_some(),
                    map: parsed.or(previous_map),
                }
            })
            .collect();
        if self.selected.is_none() && self.generated.is_some() {
            return true;
        }

        let next = {
            let mut valid = self.valid();
            valid
                .clone()
                .find(|(p, _)| Some(*p) == self.selected.as_ref())
                .or_else(|| valid.clone().find(|(p, _)| p.ends_with("switchyard.map")))
                .or_else(|| valid.next())
                .map(|(p, m)| (p.clone(), m.clone()))
        };
        if let Some((p, map)) = next {
            self.map = map;
            self.selected = Some(p);
        } else if self.selected.take().is_some() {
            self.map = self
                .generated
                .clone()
                .unwrap_or_else(|| Self::default().map);
        }
        true
    }

    fn valid(&self) -> impl Iterator<Item = (&PathBuf, &Arc<ArenaMap>)> + Clone {
        self.files
            .iter()
            .filter_map(|f| Some((&f.entry.path, f.map.as_ref()?)))
    }

    pub(super) fn files(&self) -> &[CachedMap] {
        &self.files
    }

    pub(super) fn select(&mut self, entry: &Entry) -> bool {
        let Some(map) = self
            .files
            .iter()
            .find(|f| f.entry == *entry && f.readable)
            .and_then(|f| f.map.clone())
        else {
            return false;
        };

        self.selected = Some(entry.path.clone());
        self.map = map;
        true
    }

    pub(super) fn remove_from(&mut self, dir: &Path, entry: &Entry) -> io::Result<()> {
        let entries = Folder::Maps.scan(dir)?;
        let current = entries.contains(entry);
        self.reconcile(entries);
        if !current {
            return Err(io::Error::other(tr("fileChanged")));
        }

        fs::remove_file(&entry.path)?;
        self.reconcile(Folder::Maps.scan(dir)?);
        Ok(())
    }

    pub(super) fn name(&self) -> String {
        if let Some(path) = &self.selected {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            tr("generatedMap")
        }
    }

    pub(super) fn multiple(&self) -> bool {
        self.valid().count() + usize::from(self.generated.is_some()) > 1
    }

    pub(super) fn cycle(&mut self, delta: isize) {
        let options: Vec<_> = self
            .valid()
            .map(|(p, m)| (Some(p.clone()), m.clone()))
            .chain(self.generated.iter().map(|m| (None, m.clone())))
            .collect();
        if options.len() < 2 {
            return;
        }

        let current = options
            .iter()
            .position(|(p, _)| p == &self.selected)
            .unwrap_or(0);
        let i = (current as isize + delta).rem_euclid(options.len() as isize) as usize;
        (self.selected, self.map) = options[i].clone();
    }

    pub(super) fn generate(&mut self, size: [usize; 2]) -> Result<(), tank_arena::sim::MapError> {
        let map = Arc::new(ArenaMap::generate(fastrand::u64(..), size)?);
        self.selected = None;
        self.generated = Some(map.clone());
        self.map = map;
        Ok(())
    }

    pub(super) fn can_save(&self) -> bool {
        self.selected.is_none()
    }

    pub(super) fn save_to(&mut self, dir: &Path) -> io::Result<()> {
        let (w, h) = self.map.size();
        let path = dir.join(format!("arena-{w}x{h}-{:016x}.map", fastrand::u64(..)));
        atomic::write(&path, Mode::CreateNew, |out| {
            writeln!(out, "{}", self.map.source())
        })?;
        self.generated = None;
        self.selected = Some(path);
        self.reconcile(Folder::Maps.scan(dir)?);
        Ok(())
    }
}

// 只在部署页和地图库轮询；比赛和再战都沿用开局时的地图。
pub(super) fn refresh(mut field: ResMut<Field>) {
    let f = field.bypass_change_detection();
    let (changed, error) = match Folder::Maps.path().and_then(|p| Folder::Maps.scan(&p)) {
        Ok(entries) => (f.reconcile(entries), None),
        Err(e) => (false, Some(format!("{}: {e}", tr("mapReadFailed")))),
    };
    if changed || f.scan_error != error {
        f.scan_error = error;
        field.set_changed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directory_changes_keep_selection_and_unsaved_work() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let scan = |f: &mut Field| f.reconcile(Folder::Maps.scan(dir).unwrap());
        let a = dir.join("a.map");
        let b = dir.join("b.map");
        fs::write(&a, SWITCHYARD).unwrap();
        fs::write(&b, b"partial").unwrap();
        let mut f = Field::default();
        assert!(scan(&mut f));
        assert_eq!(f.selected.as_ref(), Some(&a));
        assert!(!scan(&mut f));
        assert!(!f.multiple());
        f.generate([9, 7]).unwrap();
        let draft = f.map.clone();
        fs::write(&b, ArenaMap::generate(4, [4, 6]).unwrap().source()).unwrap();
        scan(&mut f);

        assert!(Arc::ptr_eq(&f.map, &draft));
        f.cycle(-1);

        assert_eq!(f.map.size(), (4, 6));
        fs::write(&b, b"in progress").unwrap();
        scan(&mut f);

        assert_eq!(f.map.size(), (4, 6));
        fs::write(&b, SWITCHYARD).unwrap();
        scan(&mut f);

        assert_eq!(f.map.size(), (12, 8));
        fs::remove_file(&b).unwrap();
        scan(&mut f);

        assert_eq!(f.selected.as_ref(), Some(&a));
        f.cycle(1);

        assert!(Arc::ptr_eq(&draft, &f.map));
        f.save_to(dir).unwrap();

        assert!(!f.can_save());
        assert_eq!(f.map.source(), draft.source());
        assert_eq!(
            load(f.selected.as_ref().unwrap()).unwrap().source(),
            draft.source()
        );
    }

    #[test]
    fn manager_deploys_by_version_and_never_confirms_a_replaced_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path();
        let a = dir.join("a.map");
        let b = dir.join("b.map");
        fs::write(&a, SWITCHYARD).unwrap();
        fs::write(&b, ArenaMap::generate(7, [9, 7]).unwrap().source()).unwrap();
        let mut f = Field::default();
        f.reconcile(Folder::Maps.scan(dir).unwrap());
        let before = f.files()[1].entry.clone();
        assert!(f.select(&before));
        assert_eq!(f.map.size(), (9, 7));
        fs::write(&b, "partial save").unwrap();

        assert!(f.remove_from(dir, &before).is_err());
        assert!(b.exists());
        assert!(f.files()[1].on_disk().is_none());
        assert_eq!(f.map.size(), (9, 7)); // 当前选中的地图不受未完成写入的影响。
        assert!(!f.select(&before));
        let invalid = f.files()[1].entry.clone();
        assert!(!f.select(&invalid));
        f.remove_from(dir, &invalid).unwrap();

        assert!(!b.exists());
        assert_eq!(f.selected.as_deref(), Some(a.as_path()));
        let last = f.files()[0].entry.clone();
        f.remove_from(dir, &last).unwrap();

        assert!(f.files().is_empty());
        assert!(f.selected.is_none());
        assert_eq!(f.name(), tr("generatedMap"));
        assert_ne!(f.map.source(), SWITCHYARD.trim_end());
    }
}
