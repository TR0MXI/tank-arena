use super::*;
use std::collections::HashSet;

#[test]
fn repeated_roster_edits_reuse_visible_skin_handles() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.luau");
    std::fs::write(
        &path,
        format!(
            "-- @name 翠鸟\n-- @author Ada\n--[=[tank-skin\n{}\n]=]\nreturn function() end",
            tank_arena::appearance::TEMPLATE
        ),
    )
    .unwrap();
    let mut app = artwork::tests::headless();
    let mut draft = Draft::default();
    draft.models.push(path.clone());
    app.init_resource::<tank_manager::Looks>()
        .init_resource::<Field>()
        .init_resource::<Profiles>()
        .init_resource::<locale::Preferences>()
        .insert_resource(Section::Lineup)
        .init_resource::<board::Preview>()
        .insert_resource(draft)
        .init_asset::<Mesh>()
        .init_asset::<ColorMaterial>()
        .add_systems(Startup, arena_view::camera)
        .add_systems(
            Update,
            (
                profiles::refresh,
                board::render,
                hangar::sync,
                hangar::update_destination,
            )
                .chain(),
        );
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_millis(16),
    ));
    let mut handles = HashSet::new();
    let mut entities = HashSet::new();
    let start = std::time::Instant::now();
    for i in 0..100 {
        let mut draft = app.world_mut().resource_mut::<Draft>();
        if i % 2 == 0 {
            draft.deploy(&path, TeamId(1));
        } else {
            draft.remove(SlotId(1));
        }
        app.update();
        let w = app.world_mut();
        for (id, node) in w.query::<(Entity, &ImageNode)>().iter(w) {
            handles.insert(node.image.id());
            entities.insert(id);
        }
    }

    println!(
        "100 roster edits: {} distinct image handles, {:?}",
        handles.len(),
        start.elapsed()
    );
    assert_eq!(
        handles.len(),
        2,
        "only the frame and livery should be created"
    );
    assert_eq!(
        entities.len(),
        2,
        "existing card entities should survive edits"
    );

    // 候选卡和已部署成员使用同一显示名，同名替换也同时刷新两处。
    app.world_mut()
        .resource_mut::<Draft>()
        .deploy(&path, TeamId(1));
    app.update();
    let visible_names = |w: &mut bevy::prelude::World, name: &str| {
        w.query::<&Text>()
            .iter(w)
            .filter(|text| text.0 == name)
            .count()
    };
    assert_eq!(visible_names(app.world_mut(), "翠鸟"), 2);

    // 轮询使用 Bevy 的真实时钟；替换同名脚本后，不能继续显示旧的作者外观。
    std::fs::write(&path, "-- @name 云雀\nreturn function() end").unwrap();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(RESCAN));
    app.update();
    let w = app.world_mut();
    assert_eq!(visible_names(w, "云雀"), 2);
    assert_eq!(visible_names(w, "翠鸟"), 0);
    let current = w
        .query::<&ImageNode>()
        .iter(w)
        .map(|node| node.image.id())
        .collect::<HashSet<_>>();
    assert_eq!(current.len(), 2);
    assert!(current.is_disjoint(&handles));
}
