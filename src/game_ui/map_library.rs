use super::*;
use field::CachedMap;
use tank_arena::local_files::Entry;

#[derive(Component)]
struct MapShelf;

#[derive(Resource, Default)]
struct Browser {
    page: usize,
    confirm: Option<Entry>,
    notice: Option<String>,
}

impl Browser {
    fn reconcile(&mut self, files: &[CachedMap]) {
        self.page = self.page.min(shelves::last_page(files.len()));
        if self
            .confirm
            .as_ref()
            .is_some_and(|e| !files.iter().any(|f| f.entry == *e))
        {
            self.confirm = None;
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Browser>()
        .add_systems(
            OnEnter(Page::Maps),
            (field::refresh, |mut b: ResMut<Browser>| {
                *b = Browser::default();
            }),
        )
        .add_systems(
            Update,
            (field::refresh.run_if(on_real_timer(RESCAN)), render)
                .chain()
                .run_if(in_state(Page::Maps)),
        );
}

fn render(
    mut c: Commands,
    field: Res<Field>,
    mut b: ResMut<Browser>,
    mut images: ResMut<Assets<Image>>,
    old: Query<Entity, With<MapShelf>>,
    mut thumbs: Local<Vec<(Entry, Handle<Image>)>>,
) {
    if !field.is_changed() && !b.is_changed() && !old.is_empty() {
        return;
    }

    for e in &old {
        c.entity(e).despawn();
    }

    let files = field.files();
    b.reconcile(files);
    let visible = shelves::page_items(files, b.page);
    thumbs.retain(|(entry, _)| visible.iter().any(|f| f.entry == *entry));
    c.spawn((
        MapShelf,
        DespawnOnExit(Page::Maps),
        root(),
        BackgroundColor(paper()),
    ))
    .with_children(|p| {
        shelves::Layout {
            title: "mapLibrary",
            empty: "emptyMaps",
            notice: field.scan_error.as_deref().or(b.notice.as_deref()),
            count: files.len(),
            page: b.page,
        }
        .show(
            p,
            |p| {
                for file in visible {
                    let thumb = file.on_disk().map(|map| {
                        if let Some((_, handle)) = thumbs.iter().find(|(e, _)| *e == file.entry) {
                            return handle.clone();
                        }
                        let handle = images.add(tactical_map::thumbnail(map));
                        thumbs.push((file.entry.clone(), handle.clone()));
                        handle
                    });
                    card_item(p, file, thumb, b.confirm.as_ref() == Some(&file.entry));
                }
            },
            |page| {
                observe(move |_: On<Activate>, mut b: ResMut<Browser>| {
                    b.page = page;
                    b.confirm = None;
                })
            },
        );
    });
}

fn deploy(entry: Entry) -> impl Bundle {
    observe(
        move |_: On<Activate>, mut field: ResMut<Field>, mut next: ResMut<NextState<Page>>| {
            if field.select(&entry) {
                next.set(Page::Setup);
            }
        },
    )
}

fn card_item(
    p: &mut ChildSpawnerCommands,
    file: &CachedMap,
    image: Option<Handle<Image>>,
    confirming: bool,
) {
    let entry = &file.entry;
    let name = entry.path.file_name().unwrap_or_default().to_string_lossy();
    p.spawn(shelves::file_card()).with_children(|p| {
        let mut thumb = p.spawn(shelves::thumbnail());
        if let Some(image) = image {
            thumb.insert(ImageNode::new(image));
            if !confirming {
                thumb.insert((
                    Button,
                    AccessibleLabel::new(format!("{} · {name}", tr("deployMap"))),
                    deploy(entry.clone()),
                ));
            }
        } else {
            thumb.with_child(label(tr("previewUnavailable"), 15., paper()));
        }
        p.spawn(shelves::title(&name));
        let size = file.on_disk().map_or_else(
            || tr("invalidMapFile"),
            |map| {
                let (w, h) = map.size();
                let spawns = rust_i18n::t!("spawnCount", n = map.spawns().len());
                format!("{w} × {h}   ·   {spawns}")
            },
        );
        p.spawn(label(size, 14., quiet()));
        p.spawn(spread()).with_children(|p| {
            if confirming {
                p.spawn((
                    compact(&tr("cancelDelete")),
                    observe(|_: On<Activate>, mut b: ResMut<Browser>| b.confirm = None),
                ));
                let file = entry.clone();
                p.spawn((
                    button(&tr("confirmDelete")),
                    observe(
                        move |_: On<Activate>, mut b: ResMut<Browser>, mut field: ResMut<Field>| {
                            if b.confirm.as_ref() != Some(&file) {
                                return;
                            }

                            b.notice = Folder::Maps
                                .path()
                                .and_then(|dir| field.remove_from(&dir, &file))
                                .err()
                                .map(|e| format!("{} · {e}", tr("deleteFailed")));
                            b.confirm = None;
                        },
                    ),
                ));
            } else {
                if file.on_disk().is_some() {
                    p.spawn((compact(&tr("deployMap")), deploy(entry.clone())));
                } else {
                    p.spawn(disabled(&tr("deployMap")));
                }
                let file = entry.clone();
                p.spawn((
                    compact(&tr("delete")),
                    observe(move |_: On<Activate>, mut b: ResMut<Browser>| {
                        b.confirm = Some(file.clone());
                        b.notice = None;
                    }),
                ));
            }
        });
    });
}
