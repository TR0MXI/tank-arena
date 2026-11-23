use std::{fs, path::Path, sync::Arc};
use tank_arena::{
    bot::{LuauBot, Script},
    catalog,
    sim::*,
};

fn world() -> World {
    World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        (1..=2)
            .map(|n| Seat {
                slot: SlotId(n),
                team: TeamId(n),
            })
            .collect(),
        ClockRules::default(),
    )
    .unwrap()
}
fn write(root: &Path, name: &str, text: &str) {
    let p = root.join(name);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, text).unwrap();
}

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

// Fixed observations only; neither pilot can advance a new game. Each team digest is from its own archive.
#[test]
fn modular_avalon_sixty_tick_candidate_has_stable_controls_on_fixed_replays() {
    assert_avalon_replay_controls(
        "avalon-modular",
        "3132d224e6c7bd73490cfcf483cbe35fbcbe7813ca57abd3d07db833ecdcac53",
    );
}

#[test]
fn named_avalon_pivot_keeps_archived_pivot_local_controls() {
    assert_avalon_replay_controls(
        "avalon-pivot",
        "38a5a5ccf7f2d0765ae86e5269f7a71fcf30aa543304c18b346c5b227718d014",
    );
}

fn assert_avalon_replay_controls(package: &str, team_digest: &str) {
    use tank_arena::replay::Playback;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = Script::read(
        &root
            .join("assets/strategies")
            .join(package)
            .join("bot.luau"),
    )
    .unwrap();
    for (recording, count, expected) in [
        (
            "pursuit",
            40,
            "26068a898795af5266bb1297141235a30bc7938c3788d90b183d388e753ec0d2",
        ),
        ("teams-4v4", 25, team_digest),
    ] {
        let mut p =
            Playback::open(&root.join(format!("tests/fixtures/{recording}.tankrec"))).unwrap();
        let mut bot = LuauBot::for_world(&candidate, p.world(), SlotId(1)).unwrap();
        let mut controls = Vec::new();
        while !p.ended() && p.world().tick() < 120 && p.world().tank(SlotId(1)).unwrap().alive {
            if p.world().tick() % 3 == 0 {
                controls.push(bot.decide(p.world()).unwrap());
            }
            p.advance().unwrap();
        }
        assert_eq!(controls.len(), count);
        let hash = blake3::hash(&serde_json::to_vec(&controls).unwrap());
        assert_eq!(hash.to_hex().as_str(), expected, "{recording}");
    }
}

fn copy_current_avalon_modules() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/strategies/avalon-modular");
    for folder in ["model", "safety", "perception", "tactics"] {
        let dst = dir.path().join(folder);
        fs::create_dir(&dst).unwrap();
        for entry in fs::read_dir(src.join(folder)).unwrap() {
            let entry = entry.unwrap();
            if entry.path().extension().is_some_and(|s| s == "luau") {
                fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
            }
        }
    }
    dir
}

#[test]
fn modular_avalon_keeps_a_verified_late_approach_but_does_not_ignore_an_imminent_crossing() {
    let dir = copy_current_avalon_modules();
    write(
        dir.path(),
        "bot.luau",
        include_str!("fixtures/avalon-prefix-crossing.luau"),
    );
    let w = world();
    let mut bot = LuauBot::for_world(
        &Script::read(&dir.path().join("bot.luau")).unwrap(),
        &w,
        SlotId(1),
    )
    .unwrap();
    assert_eq!(bot.decide(&w).unwrap().fire, Some(true));
}

// Probe the production risk module with a stationary pilot and a path that reaches
// it in exactly one chosen slice. Only observations are evaluated, not new games.
#[test]
fn modular_avalon_scores_the_last_slice_at_every_density_but_not_past_sixty() {
    let dir = copy_current_avalon_modules();
    write(
        dir.path(),
        "bot.luau",
        r#"
        local Frame, Risk = require('./model/frame'), require('./safety/risk')
        local Tuning, Tracks = require('./model/tuning'), require('./perception/tracks')
        return function(original)
            local o = table.clone(original)
            Frame.update(o)
            Tracks.update(o, o.tick)
            local me, enemy = o.me, o.tanks[2]
            assert(Tuning.SLICE == 3 and Tuning.HORIZON_TICKS == 60)
            o.ammo = 1
            -- Includes the low-ammo isolated duel that previously enabled a 72-tick horizon.
            o.shells = table.create(5, {x=me.x, y=me.y, owner=enemy.slot_id})
            for _, seats in {2, 8} do
                o.tanks = table.clone(original.tanks)
                for i = 3, seats do o.tanks[i] = {alive=false, slot_id=i} end
                Risk.chooseClearance(o, me)
                for _, count in {1, 3, 4, 6} do
                    local function score(arrival)
                        local paths = {steps=0, blockers={}, memo={}, detailed=false}
                        local p = {base=0, x0=me.x + arrival*o.cell, y0=me.y, armedAt=1}
                        for k = 1, 21 do p[2*k-1], p[2*k] = me.x+(arrival-k)*o.cell, me.y end
                        paths[1] = p
                        for i=2,count do paths[i]={base=0, armedAt=1} end
                        Risk.prepareRisk(o, me, paths)
                        assert(paths.steps * Tuning.SLICE == 60)
                        return Risk.score(o, me, paths, 'stop', 'none')
                    end
                    local fatal,_,_,_,_,hitTime = score(1)
                    assert(fatal >= 500000 and hitTime == 3, 'collision must stop positional credit')
                    assert(score(20) >= 500000, 'last-slice contact lost its hard penalty')
                    assert(score(21) == 0, 'read past horizon')
                end
            end
            -- Clear moves report the actual position and heading at tick 60.
            o.forward_speed = 1
            local clear = {steps=0, blockers={}, memo={}, detailed=false}
            Risk.prepareRisk(o, me, clear)
            local risk,x,y,c,s,ticks = Risk.score(o, me, clear, 'forward', 'none')
            local distance = math.sqrt((x-me.x)^2 + (y-me.y)^2)
            assert(risk == 0 and ticks == 60 and math.abs(distance-60) < 0.0001, tostring(distance))
            assert(math.abs(x-me.x-60*c) < 0.0001 and math.abs(y-me.y-60*s) < 0.0001)
            -- Reusing a cached action must return the same terminal pose/time.
            local _,cx,cy,cc,cs,ct = Risk.score(o, me, clear, 'forward', 'none')
            assert(cx==x and cy==y and cc==c and cs==s and ct==ticks)
            local m = Tracks.bySlot[enemy.slot_id]
            m.vx,m.vy,m.coast = 1,0,nil
            local ex,ey = Tracks.predict(enemy,60)
            assert(math.abs(ex-enemy.x-60)<0.0001 and ey==enemy.y, 'target stopped before tick 60')
            local past,_ = Tracks.predict(enemy,61)
            assert(ex==past, 'target forecast past horizon')
        end
    "#,
    );
    let w = world();
    LuauBot::for_world(
        &Script::read(&dir.path().join("bot.luau")).unwrap(),
        &w,
        SlotId(1),
    )
    .unwrap()
    .decide(&w)
    .unwrap();

    // A shot whose first contact is at tick 60 may be taken. One tick later is
    // outside the forecast, even when still inside the ammo/range policy.
    write(
        dir.path(),
        "bot.luau",
        r#"
        local Frame, Fire = require('./model/frame'), require('./tactics/fire')
        local Tracks = require('./perception/tracks')
        return function(original)
            local o = table.clone(original)
            local me, enemy = table.clone(o.me), table.clone(o.tanks[2])
            o.maze = string.rep(string.rep(' ', 33)..'\n', 32)..string.rep(' ',33)
            me.x, me.y, me.heading = 4.5*o.cell, 4.5*o.cell, 0
            o.me, o.tanks, o.can_fire, o.enemy_ahead = me, {me, enemy}, true, false
            o.ammo, o.shell_speed, o.shells = 5, 10, {}
            Frame.update(o)
            for _,t in {59,60,61} do
                enemy.x,enemy.y = me.x+o.muzzle+o.shell_speed*t+o.hit_radius-28, me.y
                Tracks.update(o,o.tick)
                local victim, flight = Fire.window(o, me)
                if t <= 60 then
                    assert(victim == enemy.slot_id and math.abs(t-flight)<0.0001, tostring(flight))
                else
                    assert(victim == nil, 'shot searched past tick 60')
                end
            end
        end
    "#,
    );
    LuauBot::for_world(
        &Script::read(&dir.path().join("bot.luau")).unwrap(),
        &w,
        SlotId(1),
    )
    .unwrap()
    .decide(&w)
    .unwrap();
}
