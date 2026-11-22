use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
use tank_arena::{bot::Script, catalog::discover};

#[test]
fn finds_only_top_level_files_and_direct_package_entries_without_running_them() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("tank-arena-bots-{}-{nonce}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::create_dir_all(root.join("猫 bot/helpers/nested")).unwrap();
    fs::create_dir(root.join("missing-entry")).unwrap();
    for file in [
        "猫 bot/bot.luau",
        "猫 bot.luau",
        "猫 bot/helper.luau",
        "猫 bot/helpers/nested/bot.luau",
        "missing-entry/other.luau",
        "notes.txt",
    ] {
        fs::write(root.join(file), "error('discovery must not execute this')").unwrap();
    }

    assert_eq!(
        discover(&root).unwrap(),
        [root.join("猫 bot/bot.luau"), root.join("猫 bot.luau")]
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn file_headers_override_only_the_display_name_not_the_entry_path() {
    use tank_arena::{
        catalog::tank_name,
        runner::Entrant,
        sim::{Seat, SlotId, TeamId},
    };

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("旧名.v2.luau");
    let package = dir.path().join("车库.v2");
    fs::create_dir(&package).unwrap();
    let entry = package.join("bot.luau");
    let seat = Seat {
        slot: SlotId(1),
        team: TeamId(1),
    };
    for (path, fallback) in [(&file, "旧名.v2"), (&entry, "车库.v2")] {
        fs::write(path, "-- @name   \nreturn function() end").unwrap();
        assert_eq!(Script::read(path).unwrap().name(), fallback);
        fs::write(
            path,
            concat!(
                "--!strict\r\n\n",
                "-- @name  同名车  \r\n",
                "-- @author   Ada 王\r\n",
                "-- @description 沿墙巡逻。\n",
                "error('not evaluated')",
            ),
        )
        .unwrap();
        let bot = Entrant::from_file(seat, path);
        assert_eq!(bot.name, "同名车");
        let script = bot.script.unwrap();
        assert_eq!(script.author(), Some("Ada 王"));
        assert_eq!(script.description(), Some("沿墙巡逻。"));
    }

    assert_eq!(discover(dir.path()).unwrap().len(), 2);
    assert_eq!(tank_name(Path::new("bots/bot.luau")), "bot");
    assert_eq!(tank_name(Path::new("bots/bots/bot.luau")), "bots");
}

#[test]
fn default_catalog_is_one_working_self_describing_example() {
    use std::sync::Arc;
    use tank_arena::{bot::LuauBot, sim::*};

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/bots");
    let path = root.join("example.luau");
    assert_eq!(discover(&root).unwrap(), std::slice::from_ref(&path));
    let script = Script::read(&path).unwrap();
    assert_ne!(script.name(), "example");
    assert!(script.author().is_some() && script.description().is_some());
    let world = World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        (1..=2)
            .map(|n| Seat {
                slot: SlotId(n),
                team: TeamId(n),
            })
            .collect(),
        ClockRules::default(),
    )
    .unwrap();
    assert!(
        LuauBot::for_world(&script, &world, SlotId(1))
            .unwrap()
            .decide(&world)
            .is_ok()
    );
}
