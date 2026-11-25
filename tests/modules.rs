#[path = "support/package.rs"]
mod package;
use package::{world, write};
use std::fs;
use tank_arena::{
    bot::{LuauBot, Script, catalog},
    sim::SlotId,
};

#[test]
fn relative_imports_are_bound_to_the_defining_module_and_cached_per_seat() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("分层");
    write(
        &root,
        "bot.luau",
        r#"
        local decide = require('./core/decide')
        local shared = require('./state.luau')
        return function(o)
            shared.n += 1
            return decide(o, shared.n)
        end"#,
    );
    write(
        &root,
        "core/decide.luau",
        r#"
        local state = require('../state')
        return function(o, n)
            assert(require('./../state') == state and state.n == n)
            return require('./action')(n)
        end"#,
    );
    write(
        &root,
        "core/action.luau",
        "return function(n) return {fire = n == 1} end",
    );
    write(&root, "state.luau", "return {n = 0}");
    let entry = root.join("bot.luau");
    assert_eq!(
        catalog::discover(dir.path()).unwrap(),
        std::slice::from_ref(&entry)
    );
    let s = Script::read(&entry).unwrap();
    let w = world();
    for n in [1, 2] {
        let mut bot = LuauBot::for_world(&s, &w, SlotId(n)).unwrap();
        assert_eq!(bot.decide(&w).unwrap().fire, Some(true));
        assert_eq!(bot.decide(&w).unwrap().fire, Some(false));
    }
}

#[test]
fn delayed_require_uses_frozen_bytes_and_replay_identity_covers_paths_and_contents() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let entry = root.join("bot.luau");
    let main = "return function() return {fire = require('./flag')} end";
    write(root, "bot.luau", main);
    assert_eq!(
        Script::read(&entry).unwrap().blake3(),
        blake3::hash(main.as_bytes())
    );
    write(root, "flag.luau", "return true");
    let first = Script::read(&entry).unwrap();
    write(root, "flag.luau", "return false");
    let next = Script::read(&entry).unwrap();
    assert_ne!(first.blake3(), next.blake3());
    let copy = tempfile::tempdir().unwrap();
    write(copy.path(), "flag.luau", "return false");
    write(copy.path(), "bot.luau", main);
    assert_eq!(
        Script::read(&copy.path().join("bot.luau"))
            .unwrap()
            .blake3(),
        next.blake3()
    );
    fs::rename(root.join("flag.luau"), root.join("other.luau")).unwrap();
    assert_ne!(Script::read(&entry).unwrap().blake3(), next.blake3());
    let w = world();
    for (s, flag) in [(first, true), (next, false)] {
        let mut b = LuauBot::for_world(&s, &w, SlotId(1)).unwrap();
        for _ in 0..2 {
            assert_eq!(b.decide(&w).unwrap().fire, Some(flag));
        }
    }
}

#[test]
fn require_reports_missing_and_cyclic_dependencies_without_poisoning_the_cache() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("model");
    write(dir.path(), "other.luau", "return 17");
    write(&root, "cycle/a.luau", "return require('./b')");
    write(&root, "cycle/b.luau", "return require('./a')");
    write(&root, "state.luau", "return {attempts = 0}");
    write(
        &root,
        "retry.luau",
        "local s = require('./state'); s.attempts += 1; assert(s.attempts > 1); return 17",
    );
    write(
        &root,
        "bot.luau",
        r#"return function()
        for path, expected in {['./cycle/a']='cyclic require', ['./missing']='missing.luau', ['../other']='leaves the bot package'} do
            local ok, err = pcall(require, path)
            assert(not ok and string.find(tostring(err), expected, 1, true), tostring(err))
        end
        assert(not pcall(require, './retry'))
        assert(require('./retry') == 17)
    end"#,
    );
    let w = world();
    LuauBot::for_world(
        &Script::read(&root.join("bot.luau")).unwrap(),
        &w,
        SlotId(1),
    )
    .unwrap()
    .decide(&w)
    .unwrap();
    write(&root, "flat.luau", "return require('./state')");
    let flat = Script::read(&root.join("flat.luau")).unwrap();
    assert!(
        LuauBot::for_world(&flat, &w, SlotId(1))
            .err()
            .unwrap()
            .to_string()
            .contains("not found")
    );
}
