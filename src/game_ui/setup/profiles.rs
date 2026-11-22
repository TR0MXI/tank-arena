use super::*;
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use tank_arena::{bot::Script, catalog::tank_name, local_files::Entry};

/// 只为当前页和已部署席位保留源码快照；比赛始终从文件重新加载。
#[derive(Resource, Default)]
pub(super) struct Profiles(BTreeMap<PathBuf, (Option<Entry>, Option<Script>)>);

impl Profiles {
    pub(super) fn script(&self, path: &Path) -> Option<&Script> {
        self.0.get(path)?.1.as_ref()
    }

    pub(super) fn name(&self, path: &Path) -> Cow<'_, str> {
        self.script(path)
            .map_or_else(|| tank_name(path).into(), |s| s.name().into())
    }

    pub(super) fn version(&self, path: &Path) -> Option<&Entry> {
        self.0.get(path)?.0.as_ref()
    }
}

pub(super) fn refresh(
    d: Res<Draft>,
    mut profiles: ResMut<Profiles>,
    time: Res<Time<Real>>,
    mut last: Local<Option<Duration>>,
) {
    let now = time.elapsed();
    let poll = last.is_none_or(|t| now.saturating_sub(t) >= RESCAN);
    if !poll && !d.is_changed() {
        return;
    }

    if poll {
        *last = Some(now);
    }
    let wanted: BTreeSet<_> = d
        .visible_models()
        .iter()
        .chain(d.slots().map(|(_, t)| &t.model))
        .collect();
    let cache = &mut profiles.bypass_change_detection().0;
    let old_count = cache.len();
    cache.retain(|path, _| wanted.contains(path));
    let mut changed = cache.len() != old_count;
    for path in wanted {
        let old = cache.get(path);
        if !poll && old.is_some() {
            continue;
        }

        let version = Entry::read(path).ok();
        if old.is_some_and(|(v, _)| *v == version) {
            continue;
        }

        let script = version.as_ref().and_then(|_| Script::read(path).ok());
        cache.insert(path.clone(), (version, script));
        changed = true;
    }

    if changed {
        profiles.set_changed();
    }
}
