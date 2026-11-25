use super::package::{world, write};
use std::{fs, path::Path};
use tank_arena::{
    bot::{LuauBot, Script},
    replay::Playback,
    sim::SlotId,
};

#[test]
fn modular_avalon_sixty_tick_candidate_has_stable_controls_on_fixed_replays() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let candidate = Script::read(&root.join("assets/strategies/avalon-modular/bot.luau")).unwrap();
    for (recording, count, expected) in [
        (
            "pursuit",
            40,
            "26068a898795af5266bb1297141235a30bc7938c3788d90b183d388e753ec0d2",
        ),
        (
            "teams-4v4",
            25,
            "3132d224e6c7bd73490cfcf483cbe35fbcbe7813ca57abd3d07db833ecdcac53",
        ),
    ] {
        let mut p =
            Playback::open(&root.join(format!("tests/fixtures/{recording}.tankrec"))).unwrap();
        let mut bot = LuauBot::for_world(&candidate, p.world(), SlotId(1)).unwrap();
        let mut controls = Vec::new();
        while !p.ended() && p.world().tick() < 120 && p.world().tank(SlotId(1)).unwrap().alive {
            if p.world().tick().is_multiple_of(3) {
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
        include_str!("../fixtures/avalon-prefix-crossing.luau"),
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
