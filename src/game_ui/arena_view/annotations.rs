use super::*;
use tank_arena::sim::{Event, SlotId};

#[derive(Component)]
pub(super) enum Marker {
    Vehicle(usize),
    Projectile(usize),
    Ricochet,
}

#[derive(Component, Default)]
pub(super) struct Caption(Option<SlotId>);

pub(super) fn spawn(p: &mut ChildSpawnerCommands, palette: &Palette) {
    for marker in (0..MAX_SLOTS)
        .map(Marker::Vehicle)
        .chain((0..MAX_SLOTS * usize::from(AMMO)).map(Marker::Projectile))
    {
        p.spawn((
            marker,
            Caption::default(),
            Text2d::new(""),
            TextFont {
                font_size: FontSize::Px(13.),
                ..default()
            },
            TextColor(ink()),
            Visibility::Hidden,
        ));
    }

    p.spawn((
        Marker::Ricochet,
        Mesh2d(palette.ring.clone()),
        MeshMaterial2d(palette.amber.clone()),
        Visibility::Hidden,
    ));
}

pub(super) type Markers<'w, 's> = Query<
    'w,
    's,
    (
        &'static Marker,
        &'static mut Transform,
        &'static mut Visibility,
        Option<(&'static mut Text2d, &'static mut Caption)>,
    ),
    Without<Actor>,
>;

pub(super) fn paint(
    map: &ArenaMap,
    tanks: &[Tank],
    shells: &[Shell],
    cue: Option<Event>,
    roster: bool,
    markers: &mut Markers,
) {
    for (marker, mut transform, mut vis, text) in markers {
        let target = match marker {
            Marker::Vehicle(i) if roster => tanks.get(*i).filter(|t| t.alive).map(|t| {
                (
                    t.center,
                    Vec2::new(0., -(0.29 * tile(map) + 7.)),
                    Some(t.seat.slot),
                )
            }),
            Marker::Projectile(i) if roster => shells
                .get(*i)
                .map(|s| (s.center, Vec2::new(0., 11.), Some(s.owner))),
            // 用裁判记录的触墙点，不从两张渲染快照猜反弹。暂停或定位回放后仍在同一面墙上。
            Marker::Ricochet => match cue {
                Some(Event::Bounce { at, .. }) => Some((at, Vec2::ZERO, None)),
                _ => None,
            },
            _ => None,
        };
        let Some((point, offset, id)) = target else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };

        vis.set_if_neq(Visibility::Inherited);
        transform.translation = (xy(point, map) + offset).extend(4.5);
        if let (Some(id), Some((mut text, mut caption))) = (id, text)
            && caption.0 != Some(id)
        {
            text.0 = format!("#{}", id.0);
            caption.0 = Some(id);
        }
    }
}
