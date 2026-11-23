use std::sync::Arc;
use tank_arena::{
    bot::{Fault, FaultKind, LuauBot, Script},
    sim::*,
};

fn world() -> World {
    World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        [0, 1]
            .map(|i| Seat {
                slot: SlotId(i + 1),
                team: TeamId(i),
            })
            .to_vec(),
        ClockRules::default(),
    )
    .unwrap()
}

fn load(w: &World, src: &str) -> mlua::Result<LuauBot> {
    LuauBot::for_world(&Script::from_source("test", src).unwrap(), w, SlotId(1))
}

#[test]
fn sparse_updates_and_readback_through_the_match_api() {
    let mut w = world();
    let mut bot = load(
        &w,
        r#"return function(o)
        if o.tick == 0 then
            assert(o.drive == 'stop' and o.turn == 'none' and not o.fire_down)
            return { drive = 'reverse', turn = 'left' }
        elseif o.tick == 3 then
            assert(o.drive == 'reverse' and o.turn == 'left')
            return { fire = true }
        elseif o.tick == 4 then
            assert(o.drive == 'reverse' and o.turn == 'left')
            assert(o.fire_down and not o.can_fire)
            return { turn = 'none' }
        elseif o.tick == 5 then
            assert(o.fire_down and not o.can_fire)
            return { fire = false }
        end
        assert(not o.fire_down and o.can_fire)
        assert(o.drive == 'reverse' and o.turn == 'none')
    end"#,
    )
    .unwrap();
    let slot = SlotId(1);
    for tick in 0..7 {
        if (1..3).contains(&tick) {
            w.step(&[], &[]);
            continue;
        }

        let c = bot.decide(&w).unwrap();
        if tick == 3 {
            assert_eq!(
                c,
                ControlUpdate {
                    fire: Some(true),
                    ..Default::default()
                }
            );
        }
        if tick >= 6 {
            assert_eq!(c, ControlUpdate::default());
        }
        w.step(&[(slot, c)], &[]);
    }
}

#[test]
fn state_persists_but_is_not_shared_between_bots() {
    let w = world();
    let script =
        "local n=0; return function(_) n+=1; return if n==1 then {drive='forward'} else {} end";
    let mut a = load(&w, script).unwrap();
    let mut b = load(&w, script).unwrap();
    assert_eq!(a.decide(&w).unwrap().drive, Some(Drive::Forward));
    assert_eq!(a.decide(&w).unwrap(), ControlUpdate::default());
    assert_eq!(b.decide(&w).unwrap().drive, Some(Drive::Forward));
}

#[test]
fn rejects_strings_invalid_updates_and_writes_to_observation() {
    let w = world();
    for body in [
        "return 'wait'",
        "return 'fire'",
        "return 42",
        "return {drive='left'}",
        "return {turn=false}",
        "return {fire=1}",
        "return {steer='left'}",
        "return {'forward'}",
    ] {
        let mut bot = load(&w, &format!("return function(obs) {body} end")).unwrap();
        assert_eq!(
            Fault::classify(bot.decide(&w).unwrap_err(), false).kind,
            FaultKind::Action,
            "{body}"
        );
    }

    let mut bot = load(&w, "return function(o) o.turn='left' end").unwrap();
    assert_eq!(
        Fault::classify(bot.decide(&w).unwrap_err(), false).kind,
        FaultKind::Runtime
    );
}

#[test]
fn long_finite_loops_finish_in_setup_and_decision() {
    let w = world();
    let mut bot = load(
        &w,
        r#"local function count()
            local n = 0
            while n < 100000 do n += 1 end
            return n
        end
        local initial = count()
        return function() return { fire = initial == 100000 and count() == initial } end"#,
    )
    .unwrap();
    assert_eq!(bot.decide(&w).unwrap().fire, Some(true));
}

#[test]
fn bot_can_use_more_than_four_mib() {
    let w = world();
    let mut bot = load(
        &w,
        r#"return function()
            local data = buffer.create(5 * 1024 * 1024)
            local last = buffer.len(data) - 1
            buffer.writeu8(data, last, 42)
            return { fire = buffer.readu8(data, last) == 42 }
        end"#,
    )
    .unwrap();
    assert_eq!(bot.decide(&w).unwrap().fire, Some(true));
}

#[test]
fn observation_rows_stay_readonly_when_shells_appear_and_disappear() {
    let empty = world();
    let mut one_shell = empty.clone();
    one_shell.step(
        &[(
            SlotId(1),
            ControlUpdate {
                fire: Some(true),
                ..Default::default()
            },
        )],
        &[],
    );

    assert_eq!(one_shell.shells().len(), 1);

    let mut bot = load(
        &empty,
        r#"
        local tanks, shells, first_shell
        return function(obs)
            tanks = tanks or obs.tanks
            shells = shells or obs.shells
            assert(tanks == obs.tanks and shells == obs.shells)
            assert(table.isfrozen(tanks) and table.isfrozen(tanks[1]) and table.isfrozen(shells))
            if obs.tick == 0 then
                assert(#shells == 0 and shells[1] == nil)
            else
                first_shell = first_shell or shells[1]
                assert(#shells == 1 and first_shell == shells[1] and first_shell.owner == 1)
                assert(table.isfrozen(first_shell))
            end
        end
    "#,
    )
    .unwrap();

    for snapshot in [&empty, &one_shell, &empty, &one_shell] {
        bot.decide(snapshot).unwrap();
    }
}

#[test]
fn inline_script_names_are_trimmed_and_validated() {
    let script = Script::from_source("  示例  ", "return function() end").unwrap();
    assert_eq!(script.name(), "示例");
    assert!(matches!(
        Script::from_source("  ", ""),
        Err(tank_arena::bot::SourceError::EmptyName)
    ));
}

#[test]
fn only_leading_metadata_is_used_and_text_is_bounded_at_character_boundaries() {
    let s = Script::from_source(
        "fallback",
        &format!(
            "-- @author {}\nlocal text = [[\n-- @name Fake\n-- @description Fake\n]]",
            "猫".repeat(200)
        ),
    )
    .unwrap();
    assert_eq!(s.name(), "fallback");
    assert_eq!(s.author().unwrap().chars().count(), 48);
    assert_eq!(s.description(), None);
    let s = Script::from_source(
        "fallback",
        "--[=[\n-- @name Fake\n]=]\nreturn function() end",
    )
    .unwrap();
    assert_eq!(s.name(), "fallback");
}

#[test]
fn me_is_the_readonly_roster_row_not_a_slot_index() {
    let world = World::new(
        Arc::new(SWITCHYARD.parse().unwrap()),
        vec![
            Seat {
                slot: SlotId(8),
                team: TeamId(3),
            },
            Seat {
                slot: SlotId(2),
                team: TeamId(1),
            },
        ],
        ClockRules::unlimited(),
    )
    .unwrap();
    let script = Script::from_source(
        "本人",
        r#"return function(obs)
        assert(obs.tanks[obs.slot_id] == nil)
        assert(obs.me == obs.tanks[2] and table.isfrozen(obs.me))
        assert(obs.me.slot_id == obs.slot_id and obs.me.team_id == obs.team_id)
    end"#,
    )
    .unwrap();
    LuauBot::for_world(&script, &world, SlotId(8))
        .unwrap()
        .decide(&world)
        .unwrap();
}

#[test]
fn behavior_seed_is_read_only_per_seat_and_exact_in_luau() {
    let w = world();
    assert_eq!(w.pilot_seed(SlotId(1)), None);
    let w = w.with_behavior_seed(Some(u64::MAX));
    assert_ne!(w.pilot_seed(SlotId(1)), w.pilot_seed(SlotId(2)));
    for slot in [SlotId(1), SlotId(2)] {
        let seed = w.pilot_seed(slot).unwrap();
        let source = format!(
            r#"return function(o)
                assert(o.behavior_seed == {seed})
                assert(not pcall(function() o.behavior_seed = 42 end))
            end"#
        );
        let script = Script::from_source("seed", &source).unwrap();
        let mut bot = LuauBot::for_world(&script, &w, slot).unwrap();
        bot.decide(&w).unwrap();
    }
}
