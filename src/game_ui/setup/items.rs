use super::*;

/// Selection and enablement are independent, so a disabled crate can still be inspected.
pub(super) fn manager(p: &mut ChildSpawnerCommands, d: &Draft, pool: ItemPool) {
    let item = d.inspecting;
    let enabled = pool.contains(item);
    let name = |k: Item| tr(k.key());
    let count = Item::ALL.iter().filter(|&&k| pool.contains(k)).count();
    p.spawn((
        Node {
            width: percent(100),
            padding: UiRect::axes(px(16), px(10)),
            border_radius: BorderRadius::all(px(16)),
            ..column(6.)
        },
        BackgroundColor(card_bg()),
    ))
    .with_children(|p| {
        p.spawn(spread()).with_children(|p| {
            p.spawn(headline(
                &format!("{}   {count}/{}", tr("crateManager"), Item::ALL.len()),
                20.,
            ));
            p.spawn(label(tr("cratePoolHint"), 14., quiet()));
        });
        p.spawn(row()).with_children(|p| {
            for k in Item::ALL {
                let text = if pool.contains(k) {
                    name(k)
                } else {
                    format!("− {}", name(k))
                };
                p.spawn(toggle(&text, item == k, move |d| d.inspecting = k));
            }
        });
        p.spawn(spread()).with_children(|p| {
            p.spawn(column(2.)).with_children(|p| {
                p.spawn(label(
                    format!(
                        "[{}] {} · {}",
                        item.symbol(),
                        name(item),
                        tr(&format!("{}Info", item.key()))
                    ),
                    16.,
                    ink(),
                ));
                p.spawn(label(stats(item), 14., quiet()));
            });
            p.spawn((
                compact(&tr(if enabled {
                    "disableCrate"
                } else {
                    "enableCrate"
                })),
                observe(
                    move |_: On<Activate>,
                          mut prefs: ResMut<locale::Preferences>,
                          mut d: ResMut<Draft>| {
                        if let Err(e) = prefs.toggle_crate(item) {
                            d.note = format!("{}: {e}", tr("saveError"));
                        }
                    },
                ),
            ));
        });
    });
}
fn stats(item: Item) -> String {
    let ammo = || format!("{} {}", tr("crateAmmo"), item.rounds());
    match item {
        Item::Laser => format!(
            "{} · {LASER_SPEED} px/s · {:.1} s",
            ammo(),
            f32::from(LASER_LIFE) / HZ as f32
        ),
        Item::Double => format!(
            "{} · {} px/s · {} s",
            ammo(),
            SIZES.shell_speed * HZ as i32 / PX,
            u32::from(DOUBLE_LIFE) / HZ
        ),
        Item::Shotgun => format!(
            "{} · {} × {PELLETS} · {} s / {}",
            ammo(),
            tr("pellets"),
            SHOTGUN_RELOAD / u64::from(HZ),
            tr("volley")
        ),
        Item::Missile => format!(
            "{} · {} s {} · {} s",
            ammo(),
            u32::from(SEEK_DELAY) / HZ,
            tr("untilHoming"),
            u32::from(MISSILE_LIFE) / HZ
        ),
        Item::Mine => format!(
            "{} · {:.1} s {} · {} × {FRAGMENTS}",
            ammo(),
            f32::from(MINE_ARM) / HZ as f32,
            tr("untilArmed"),
            tr("shards")
        ),
        Item::Gatling => format!(
            "{} · {:.1} s {} · {:.1} s / {}",
            ammo(),
            GATLING_WARMUP as f32 / HZ as f32,
            tr("warmup"),
            GATLING_INTERVAL as f32 / HZ as f32,
            tr("shot")
        ),
        Item::Frag => format!(
            "{} · {} × {FRAGMENTS} · {:.1} s {} · {} s {}",
            ammo(),
            tr("shards"),
            f32::from(FUSE) / HZ as f32,
            tr("fuse"),
            u32::from(FRAG_LIFE) / HZ,
            tr("maxLife")
        ),
        Item::Shield => format!(
            "{} {} s · {} {} px",
            tr("duration"),
            SHIELD_TICKS / u64::from(HZ),
            tr("radius"),
            SHIELD_RADIUS / PX
        ),
        Item::Speed => format!(
            "{} {} s · +{BOOST_PERCENT}%",
            tr("duration"),
            BOOST_TICKS / u64::from(HZ)
        ),
    }
}
