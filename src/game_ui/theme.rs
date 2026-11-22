//! 菜单使用浅色工作台；实况控件使用半透明深底，不依赖战场本身的颜色对比。

use super::*;
use bevy::text::{FontSource, FontWeight};

pub(super) fn ink() -> Color {
    Color::srgb_u8(24, 41, 40)
}

pub(super) fn paper() -> Color {
    Color::srgb_u8(241, 243, 233)
}

pub(super) fn quiet() -> Color {
    Color::srgb_u8(75, 93, 88)
}

pub(super) fn lime() -> Color {
    Color::srgb_u8(198, 242, 123)
}

pub(super) fn hud_bg() -> Color {
    Color::srgba_u8(18, 36, 38, 239)
}

fn text(value: impl Into<String>, size: f32, color: Color, weight: u16) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font: FontSource::families("Helvetica Neue, PingFang SC, sans-serif"),
            font_size: FontSize::Px(size),
            weight: FontWeight(weight),
            ..default()
        },
        TextColor(color),
    )
}

pub(super) fn label(value: impl Into<String>, size: f32, color: Color) -> impl Bundle {
    text(value, size, color, 450)
}

pub(super) fn headline(value: &str, size: f32) -> impl Bundle {
    text(value, size, ink(), 740)
}

pub(super) fn eyebrow(value: &str) -> impl Bundle {
    text(value, 14., quiet(), 660)
}

#[derive(Component)]
#[require(ButtonMotion)]
pub(super) struct Skin(Color, Color);

pub(super) fn surface(base: Color, over: Color) -> impl Bundle {
    (Hovered::default(), Skin(base, over), BackgroundColor(base))
}

#[derive(Component, Default)]
pub(super) struct ButtonMotion {
    pub(super) hover: f32,
    pub(super) press: f32,
}

fn control(title: &str, base: Color, over: Color, fg: Color, h: f32, size: f32) -> impl Bundle {
    (
        Button,
        surface(base, over),
        Node {
            height: px(h),
            padding: UiRect::horizontal(px(if h >= 40. { 21 } else { 12 })),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        // 在标签上按下、在内边距上松开，必须命中同一个按钮。
        children![(text(title, size, fg, 620), Pickable::IGNORE)],
    )
}

pub(super) fn primary(s: &str) -> impl Bundle {
    control(s, lime(), Color::srgb_u8(214, 255, 153), ink(), 44., 17.)
}

pub(super) fn button(s: &str) -> impl Bundle {
    control(s, ink(), quiet(), Color::WHITE, 42., 16.)
}

pub(super) fn compact(s: &str) -> impl Bundle {
    control(
        s,
        Color::srgb_u8(222, 230, 218),
        Color::srgb_u8(201, 216, 198),
        ink(),
        36.,
        15.,
    )
}

pub(super) fn dark(s: &str) -> impl Bundle {
    control(
        s,
        Color::srgb_u8(45, 67, 65),
        Color::srgb_u8(65, 90, 83),
        Color::WHITE,
        36.,
        15.,
    )
}

pub(super) fn choice(s: &str, on: bool) -> impl Bundle {
    if on {
        control(s, ink(), quiet(), Color::WHITE, 36., 15.)
    } else {
        control(
            s,
            Color::srgb_u8(230, 235, 225),
            Color::srgb_u8(213, 223, 209),
            quiet(),
            36.,
            15.,
        )
    }
}

/// 只有一页时不显示分页控件；多页时，即使翻到首尾也不移动点击区。
pub(super) fn pager<B: Bundle>(
    p: &mut ChildSpawnerCommands,
    index: usize,
    count: usize,
    go: impl Fn(usize) -> B,
) {
    if count < 2 {
        return;
    }

    p.spawn(row()).with_children(|p| {
        if index > 0 {
            p.spawn((
                compact("‹"),
                AccessibleLabel::new(tr("newer")),
                go(index - 1),
            ));
        } else {
            p.spawn(disabled("‹"));
        }
        p.spawn(eyebrow(&format!("{} / {count}", index + 1)));
        if index + 1 < count {
            p.spawn((
                compact("›"),
                AccessibleLabel::new(tr("older")),
                go(index + 1),
            ));
        } else {
            p.spawn(disabled("›"));
        }
    });
}

pub(super) fn disabled(s: &str) -> impl Bundle {
    unavailable(s, 36., 15.)
}

pub(super) fn disabled_primary(s: &str) -> impl Bundle {
    unavailable(s, 44., 17.)
}

fn unavailable(s: &str, h: f32, size: f32) -> impl Bundle {
    (
        Node {
            height: px(h),
            padding: UiRect::horizontal(px(if h >= 40. { 21 } else { 12 })),
            align_items: AlignItems::Center,
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(232, 235, 227)),
        children![text(s, size, quiet(), 620)],
    )
}

type Controls<'w, 's> = Query<
    'w,
    's,
    (
        &'static Hovered,
        Has<bevy::ui::Pressed>,
        &'static Skin,
        &'static mut ButtonMotion,
        &'static mut BackgroundColor,
    ),
>;

pub(super) fn hover(time: Res<Time>, mut controls: Controls) {
    let dt = time.delta_secs();
    for (hover, pressed, skin, mut motion, mut color) in &mut controls {
        let (over, down) = (u8::from(hover.get()) as f32, u8::from(pressed) as f32);
        if motion.hover == over && motion.press == down {
            continue;
        }

        let approach = |x: f32, target: f32, speed: f32| {
            let next = target + (x - target) * (-dt * speed).exp();
            if (next - target).abs() < 0.002 {
                target
            } else {
                next
            }
        };
        motion.hover = approach(motion.hover, over, 23.);
        motion.press = approach(motion.press, down, 45.);
        color.0 = skin
            .0
            .mix(&skin.1, motion.hover)
            .mix(&ink(), 0.22 * motion.press);
    }
}

pub(super) fn ease(t: f32) -> f32 {
    t * t * (3. - 2. * t)
}

/// 只给不可交互的页面插图和标题加动画，不移动控件或实况信息栏。
#[derive(Component)]
pub(super) struct PageIntro;

type IntroItems<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut UiTransform,
        Option<&'static mut TextColor>,
        Option<&'static mut ImageNode>,
    ),
    With<PageIntro>,
>;

pub(super) fn page_intro(
    page: Res<State<Page>>,
    time: Res<Time<Real>>,
    mut entered: Local<Duration>,
    mut items: IntroItems,
) {
    if page.is_changed() {
        *entered = time.elapsed();
    }
    let p = ease((time.elapsed().saturating_sub(*entered).as_secs_f32() / 0.20).min(1.));
    for (mut pose, text, image) in &mut items {
        let offset = Val2::px(0., (1. - p) * 5.);
        if pose.translation != offset {
            pose.translation = offset;
        }
        let alpha = 0.55 + 0.45 * p;
        if let Some(mut text) = text
            && text.0.alpha() != alpha
        {
            text.0.set_alpha(alpha);
        }
        if let Some(mut image) = image
            && image.color.alpha() != alpha
        {
            image.color.set_alpha(alpha);
        }
    }
}

pub(super) fn root() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(0),
        top: px(0),
        width: vw(100),
        max_width: vw(100),
        min_width: px(0),
        height: vh(100),
        max_height: vh(100),
        min_height: px(0),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}

pub(super) fn column(gap: f32) -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(gap),
        align_items: AlignItems::FlexStart,
        ..default()
    }
}

pub(super) fn row() -> Node {
    Node {
        column_gap: px(8),
        row_gap: px(4),
        align_items: AlignItems::Center,
        flex_wrap: FlexWrap::Wrap,
        ..default()
    }
}

pub(super) fn spread() -> Node {
    Node {
        width: percent(100),
        justify_content: JustifyContent::SpaceBetween,
        ..row()
    }
}

/// 导航锚点与内容互为兄弟节点，不放入动画或可换行的容器。
pub(super) fn page_body() -> Node {
    Node {
        width: percent(100),
        padding: UiRect {
            left: px(32),
            right: px(32),
            top: px(22),
            bottom: px(22),
        },
        ..column(18.)
    }
}

pub(super) fn page_header() -> Node {
    Node {
        width: percent(100),
        height: px(46),
        padding: UiRect::left(px(54)),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        ..default()
    }
}

pub(super) fn beside_menu() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(82),
        top: px(22),
        height: px(46),
        padding: UiRect::horizontal(px(12)),
        border_radius: BorderRadius::all(px(12)),
        ..row()
    }
}

pub(super) fn card_bg() -> Color {
    Color::srgb_u8(250, 251, 245)
}

pub(super) fn card(width: f32) -> impl Bundle {
    (
        Node {
            width: percent(width),
            padding: UiRect::all(px(20)),
            border_radius: BorderRadius::all(px(16)),
            ..column(10.)
        },
        BackgroundColor(card_bg()),
    )
}

pub(super) const ICON_SIZE: f32 = 46.;

pub(super) fn bottom_dock() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            bottom: px(9),
            left: px(12),
            padding: UiRect::axes(px(12), px(5)),
            border_radius: BorderRadius::all(px(10)),
            ..row()
        },
        BackgroundColor(hud_bg()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_labels_do_not_split_a_click_into_two_hit_targets() {
        let mut w = World::new();
        let id = w.spawn(primary("Start")).id();
        assert!(w.get::<Button>(id).is_some());
        let glyph = w.get::<Children>(id).unwrap()[0];
        let pick = w
            .get::<Pickable>(glyph)
            .expect("label must pass pointer hits to its button");
        assert!(!pick.should_block_lower && !pick.is_hoverable);
    }
}
