mod thumbnails;

use super::*;
use std::{fs, io};
use tank_arena::local_files::Entry;
use thumbnails::{Covers, Thumbnail};

#[derive(Resource, Default)]
pub(super) struct Library {
    entries: Vec<Entry>,
    scan_error: Option<String>,
    page: usize,
    confirm: Option<Entry>,
    pub(super) notice: Option<String>,
}

#[derive(Component)]
pub(super) struct Shelf;

impl Library {
    fn refresh(&mut self, result: io::Result<Vec<Entry>>) -> bool {
        let mut entries = match result {
            Ok(entries) => entries,
            Err(e) => {
                let error = Some(locale::replay_error(&e));
                if self.scan_error == error {
                    return false;
                }
                self.scan_error = error;
                return true;
            }
        };
        entries.reverse();
        let recovered = self.scan_error.take().is_some();
        if entries == self.entries {
            return recovered;
        }

        // 即使路径相同，外部替换过文件后也必须重新确认。
        if self
            .confirm
            .as_ref()
            .is_some_and(|entry| !entries.contains(entry))
        {
            self.confirm = None;
        }
        self.page = self.page.min(shelves::last_page(entries.len()));
        self.entries = entries;
        true
    }
}

fn files() -> io::Result<Vec<Entry>> {
    Folder::Replays.scan(&Folder::Replays.path()?)
}

pub(super) fn scan(mut lib: ResMut<Library>) {
    lib.page = 0;
    lib.confirm = None;
    lib.refresh(files());
}

pub(super) fn refresh(mut lib: ResMut<Library>) {
    if lib.bypass_change_detection().refresh(files()) {
        lib.set_changed();
    }
}

fn remove(entry: &Entry) -> io::Result<()> {
    if !entry.is_current() {
        return Err(io::Error::other(tr("fileChanged")));
    }

    let path = &entry.path;
    fs::remove_file(path)?;
    match fs::remove_file(path.with_extension("result.json")) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        v => v,
    }
}

fn card_item(p: &mut ChildSpawnerCommands, entry: &Entry, confirm: bool) {
    let path = &entry.path;
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let title = stem
        .strip_prefix("match-")
        .and_then(|s| s.split('-').next())
        .and_then(|s| s.parse().ok())
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d   ·   %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|| stem.into_owned());
    p.spawn(shelves::file_card()).with_children(|p| {
        let thumb = p.spawn(shelves::thumbnail()).id();
        if !confirm {
            p.commands().entity(thumb).insert((
                Button,
                AccessibleLabel::new(format!("{} · {}", tr("play"), title)),
                open(path.to_owned()),
            ));
        }
        p.spawn(shelves::title(&title));
        let caption = p
            .spawn((
                label("", 14., quiet()),
                TextLayout::no_wrap(),
                Node {
                    height: px(20),
                    max_width: percent(100),
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .id();
        p.commands()
            .entity(thumb)
            .insert(Thumbnail(entry.clone(), caption));
        p.spawn(Node {
            width: percent(100),
            justify_content: JustifyContent::SpaceBetween,
            ..row()
        })
        .with_children(|p| {
            if confirm {
                p.spawn((
                    compact(&tr("cancelDelete")),
                    observe(|_: On<Activate>, mut l: ResMut<Library>| l.confirm = None),
                ));
                let file = entry.clone();
                p.spawn((
                    button(&tr("confirmDelete")),
                    observe(move |_: On<Activate>, mut l: ResMut<Library>| {
                        if l.confirm.as_ref() != Some(&file) {
                            return;
                        }

                        l.notice = remove(&file)
                            .err()
                            .map(|e| format!("{} · {e}", tr("deleteFailed")));
                        l.confirm = None;
                        l.refresh(files());
                    }),
                ));
            } else {
                p.spawn((compact(&tr("play")), open(path.to_owned())));
                let file = entry.clone();
                p.spawn((
                    compact(&tr("delete")),
                    observe(move |_: On<Activate>, mut l: ResMut<Library>| {
                        l.confirm = Some(file.clone());
                    }),
                ));
            }
        });
    });
}

pub(super) fn shelf(
    mut c: Commands,
    lib: Res<Library>,
    old: Query<Entity, With<Shelf>>,
    mut covers: ResMut<Covers>,
) {
    if !lib.is_changed() && !old.is_empty() {
        return;
    }

    for e in &old {
        c.entity(e).despawn();
    }

    let visible = shelves::page_items(&lib.entries, lib.page);
    covers.show(visible);
    c.spawn((
        Shelf,
        DespawnOnExit(Page::Library),
        root(),
        BackgroundColor(paper()),
    ))
    .with_children(|p| {
        shelves::Layout {
            title: "library",
            empty: "emptyLibrary",
            notice: lib.scan_error.as_deref().or(lib.notice.as_deref()),
            count: lib.entries.len(),
            page: lib.page,
        }
        .show(
            p,
            |p| {
                for entry in visible {
                    card_item(p, entry, lib.confirm.as_ref() == Some(entry));
                }
            },
            |page| {
                observe(move |_: On<Activate>, mut l: ResMut<Library>| {
                    l.page = page.min(shelves::last_page(l.entries.len()));
                    l.confirm = None;
                })
            },
        );
    });
}

pub(super) fn install(app: &mut App) {
    thumbnails::install(app);
    app.init_resource::<Library>()
        .add_systems(OnEnter(Page::Library), scan)
        .add_systems(OnExit(Page::Library), |mut l: ResMut<Library>| {
            l.notice = None;
        })
        .add_systems(
            Update,
            (refresh.run_if(on_real_timer(RESCAN)), shelf)
                .chain()
                .run_if(in_state(Page::Library)),
        );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_between_confirm_and_delete_preserves_record_and_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("match.tankrec");
        let side = path.with_extension("result.json");
        fs::write(&path, b"old").unwrap();
        let confirmed = Entry::read(&path).unwrap();
        let replacement = dir.path().join("replacement");
        fs::write(&replacement, b"replacement").unwrap();
        fs::rename(replacement, &path).unwrap();
        fs::write(&side, b"new").unwrap();

        assert!(remove(&confirmed).is_err());
        assert_eq!(fs::read(path).unwrap(), b"replacement");
        assert_eq!(fs::read(side).unwrap(), b"new");
    }

    #[test]
    fn deleting_also_removes_sidecar_but_not_unrelated_items() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("demo");
        fs::create_dir(&dir).unwrap();
        let copied = dir.join("test.tankrec");
        fs::write(&copied, b"test").unwrap();
        let sidecar = copied.with_extension("result.json");
        fs::write(&sidecar, "{}").unwrap();
        let other = dir.join("other.txt");
        fs::write(&other, "keep").unwrap();
        remove(&Entry::read(&copied).unwrap()).unwrap();

        assert!(!sidecar.exists());
        assert!(other.exists());
        assert!(Folder::Replays.scan(&dir).unwrap().is_empty());
    }
}
