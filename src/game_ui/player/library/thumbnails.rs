use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use std::collections::HashMap;

// 任务与卡片实体分离；确认弹窗导致卡片重建时不重新解码。仅保留当前页。
#[derive(Resource, Default)]
pub(in crate::game_ui::player) struct Covers(HashMap<Entry, Cover>);

enum Cover {
    Pending(Task<io::Result<(Image, String)>>),
    Ready(Option<(Handle<Image>, String)>),
}

impl Covers {
    pub(super) fn show(&mut self, visible: &[Entry]) {
        self.0.retain(|entry, _| visible.contains(entry));
        for entry in visible {
            self.0.entry(entry.clone()).or_insert_with(|| {
                let path = entry.path.clone();
                Cover::Pending(AsyncComputeTaskPool::get().spawn(async move {
                    let (h, world) = Playback::preview(&path)?;
                    Ok((
                        tactical_map::replay(&world),
                        h.roster
                            .iter()
                            .map(|b| b.name.as_str())
                            .collect::<Vec<_>>()
                            .join("  /  "),
                    ))
                }))
            });
        }
    }
}

#[derive(Component)]
pub(super) struct Thumbnail(pub Entry, pub Entity);

fn apply(
    mut c: Commands,
    mut covers: ResMut<Covers>,
    waiting: Query<(Entity, &Thumbnail)>,
    mut images: ResMut<Assets<Image>>,
    mut texts: Query<&mut Text>,
) {
    for (entity, Thumbnail(key, caption)) in &waiting {
        let Some(cover) = covers.0.get_mut(key) else {
            continue;
        };

        if let Cover::Pending(task) = cover {
            let Some(done) = check_ready(task) else {
                continue;
            };
            *cover = Cover::Ready(done.ok().map(|(img, names)| (images.add(img), names)));
        }
        if let Cover::Ready(result) = cover {
            let value = if let Some((image, names)) = result {
                c.entity(entity).insert(ImageNode::new(image.clone()));
                names.clone()
            } else {
                tr("previewUnavailable")
            };
            if let Ok(mut text) = texts.get_mut(*caption) {
                text.0 = value;
            }
            c.entity(entity).remove::<Thumbnail>();
        }
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<Covers>()
        .add_systems(
            Update,
            apply.after(super::shelf).run_if(in_state(Page::Library)),
        )
        .add_systems(OnExit(Page::Library), |mut cache: ResMut<Covers>| {
            cache.0.clear();
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn card(app: &mut App, entry: &Entry) -> (Entity, Entity) {
        let w = app.world_mut();
        w.resource_mut::<Covers>().show(std::slice::from_ref(entry));
        let caption = w.spawn(Text::default()).id();
        (w.spawn(Thumbnail(entry.clone(), caption)).id(), caption)
    }

    #[test]
    fn rebuilt_card_reuses_cover_but_replaced_file_and_empty_page_evict_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slot.tankrec");
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/first-duel.tankrec");
        fs::copy(fixture, &path).unwrap();
        let entry = Entry::read(&path).unwrap();
        let mut app = artwork::tests::headless();
        app.init_resource::<Covers>().add_systems(Update, apply);
        let (first, caption) = card(&mut app, &entry);
        let end = Instant::now() + Duration::from_secs(5);
        while app.world().get::<Thumbnail>(first).is_some() {
            assert!(Instant::now() < end);
            app.update();
            std::thread::sleep(Duration::from_millis(1));
        }

        let image = app.world().get::<ImageNode>(first).unwrap().image.clone();
        let names = app.world().get::<Text>(caption).unwrap().0.clone();
        assert!(!names.is_empty());
        app.world_mut().despawn(first);
        app.world_mut().despawn(caption);
        let (rebuilt, caption) = card(&mut app, &entry);
        assert!(matches!(
            app.world().resource::<Covers>().0[&entry],
            Cover::Ready(_)
        ));
        app.update();

        assert_eq!(app.world().get::<ImageNode>(rebuilt).unwrap().image, image);
        assert_eq!(app.world().get::<Text>(caption).unwrap().0, names);
        fs::write(&path, b"invalid replacement").unwrap();
        let replacement = Entry::read(&path).unwrap();
        card(&mut app, &replacement);
        let mut cache = app.world_mut().resource_mut::<Covers>();
        assert_eq!(cache.0.len(), 1);
        assert!(matches!(cache.0.get(&replacement), Some(Cover::Pending(_))));
        cache.show(&[]);
        assert!(cache.0.is_empty());
    }
}
