mod guides;
mod view;

use super::*;
use bevy::ui::RelativeCursorPosition;
use std::path::Path;

const PREVIEW_TEAM: [u8; 3] = [55, 139, 151];
const BRUSHES: [&str; PALETTE.len()] = [
    "erasePixel",
    "rubberPixel",
    "steelPixel",
    "sagePixel",
    "ivoryPixel",
    "teamPixel",
    "brassPixel",
    "navyPixel",
    "royalPixel",
    "silverPixel",
    "goldPixel",
    "jadePixel",
    "slateBluePixel",
];

#[derive(Clone, Copy, PartialEq)]
enum Shelf {
    Target,
    Designs,
}

#[derive(Resource)]
struct Editor {
    models: Vec<PathBuf>,
    shelf: Shelf,
    target: usize,
    saved: usize,
    note: String,
}

impl Editor {
    fn model(&self) -> Option<&Path> {
        self.models.get(self.target).map(PathBuf::as_path)
    }
}

#[derive(Resource)]
struct Brush(usize);

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Density {
    #[default]
    Simple,
    Fine,
}

impl Density {
    fn for_skin(skin: &PixelSkin) -> Self {
        // 任一 2×2 色块含单格细节，就用高级网格打开，避免第一笔误盖纹样。
        let (rows, _) = skin.cells().as_chunks::<WIDTH>();
        let (row_pairs, _) = rows.as_chunks::<2>();
        let simple = row_pairs.iter().all(|[top, bottom]| {
            top == bottom
                && top
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|[left, right]| left == right)
        });
        if simple { Self::Simple } else { Self::Fine }
    }

    fn stride(self) -> usize {
        match self {
            Self::Simple => SCALE,
            Self::Fine => 1,
        }
    }

    fn size(self) -> IVec2 {
        IVec2::new(
            (WIDTH / self.stride()) as i32,
            (HEIGHT / self.stride()) as i32,
        )
    }

    fn span(self, x: usize) -> std::ops::Range<usize> {
        let start = x * self.stride();
        start..start + self.stride()
    }
}

#[derive(Resource)]
struct Tools {
    density: Density,
    guides: bool,
}

#[derive(Resource, PartialEq)]
struct Eligible(bool);

#[derive(Component)]
struct TeamCount;

#[derive(Component)]
struct ToolBar;

#[derive(Resource)]
struct Drawing(PixelSkin);

#[derive(Component)]
struct Canvas;

#[derive(Component)]
enum DraftImage {
    Canvas,
    Preview,
}

#[derive(Component)]
struct FineLines;

#[derive(Component)]
struct Inspector;

#[derive(Component)]
struct Swatches;

#[derive(Component)]
struct Feedback;

pub(super) fn install(app: &mut App) {
    app.add_systems(OnEnter(Page::Tanks), enter).add_systems(
        Update,
        (stroke, repaint, view::tools, view::brushes, view::inspect)
            .chain()
            .run_if(in_state(Page::Tanks)),
    );
}

fn enter(mut c: Commands, looks: Res<Looks>) {
    let scan = tank_arena::bot::catalog::discover(&looks.root);
    let note = looks
        .error
        .as_ref()
        .cloned()
        .or_else(|| scan.as_ref().err().map(ToString::to_string))
        .unwrap_or_default();
    let editor = Editor {
        models: scan.unwrap_or_default(),
        shelf: Shelf::Target,
        target: 0,
        saved: 0,
        note,
    };
    let draft = editor
        .model()
        .and_then(|p| looks.preview(p))
        .unwrap_or_else(PixelSkin::template);
    c.insert_resource(editor);
    c.insert_resource(Brush(5));
    c.insert_resource(Tools {
        density: Density::for_skin(&draft),
        guides: false,
    });
    c.insert_resource(Eligible(draft.valid()));
    c.insert_resource(Drawing(draft));
    view::spawn(&mut c);
}

// 在相邻指针采样点之间补画，避免快速拖动留下空隙。
fn stroke(
    mouse: Res<ButtonInput<MouseButton>>,
    pos: Single<&RelativeCursorPosition, With<Canvas>>,
    selected: Res<Brush>,
    tools: Res<Tools>,
    mut drawing: ResMut<Drawing>,
    mut last: Local<Option<IVec2>>,
) {
    let brush = if mouse.pressed(MouseButton::Right) {
        b'.'
    } else if mouse.pressed(MouseButton::Left) {
        PALETTE[selected.0]
    } else {
        *last = None;
        return;
    };
    let Some(uv) = pos.normalized.filter(|_| pos.cursor_over()) else {
        *last = None;
        return;
    };

    let size = tools.density.size();
    let point = ((uv + 0.5) * size.as_vec2())
        .floor()
        .as_ivec2()
        .clamp(IVec2::ZERO, size - IVec2::ONE);
    let start = if tools.is_changed()
        || mouse.just_pressed(MouseButton::Left)
        || mouse.just_pressed(MouseButton::Right)
    {
        point
    } else {
        last.unwrap_or(point)
    };
    let steps = (point - start).abs().max_element().max(1);
    for i in 0..=steps {
        let p = start
            .as_vec2()
            .lerp(point.as_vec2(), i as f32 / steps as f32)
            .round()
            .as_uvec2();
        let (x, y) = (p.x as usize, p.y as usize);
        for row in tools.density.span(y) {
            for col in tools.density.span(x) {
                if drawing.0.cells()[row * WIDTH + col] != brush {
                    drawing.0.set(col, row, brush);
                }
            }
        }
    }

    *last = Some(point);
}

fn checker_pixel(v: u8, index: usize) -> [u8; 4] {
    let pixel = rgba(v, PREVIEW_TEAM);
    if pixel[3] != 0 {
        pixel
    } else if index.is_multiple_of(2) {
        [226, 232, 221, 255]
    } else {
        [214, 222, 210, 255]
    }
}

fn thumbnail(s: &PixelSkin, images: &mut Assets<Image>) -> Handle<Image> {
    images.add(arena_view::pixel_image(
        WIDTH,
        HEIGHT,
        s.cells()
            .iter()
            .flat_map(|&p| rgba(p, PREVIEW_TEAM))
            .collect(),
    ))
}

fn repaint(
    d: Res<Drawing>,
    tools: Res<Tools>,
    mut eligible: ResMut<Eligible>,
    mut nodes: Query<(&DraftImage, &mut ImageNode)>,
    mut images: ResMut<Assets<Image>>,
    mut count: Single<(&mut Text, &mut TextColor), With<TeamCount>>,
) {
    if !d.is_changed() && !tools.is_changed() {
        return;
    }

    // 用一张最近邻采样纹理代替 1,536 个界面实体；网格线与碰撞体独立叠加。
    let stride = tools.density.stride();
    for (kind, mut node) in &mut nodes {
        match kind {
            DraftImage::Canvas => {
                let data =
                    d.0.cells()
                        .iter()
                        .enumerate()
                        .flat_map(|(i, &v)| {
                            checker_pixel(v, (i % WIDTH) / stride + (i / WIDTH) / stride)
                        })
                        .collect();
                node.image = images.add(arena_view::pixel_image(WIDTH, HEIGHT, data));
            }
            DraftImage::Preview if d.is_changed() => node.image = thumbnail(&d.0, &mut images),
            DraftImage::Preview => (),
        }
    }

    if d.is_changed() {
        let n = d.0.team_pixels();
        eligible.set_if_neq(Eligible(n >= MIN_TEAM_PIXELS));
        count.0.0 = rust_i18n::t!("teamCoverage", n = n, min = MIN_TEAM_PIXELS).into_owned();
        count.1.0 = if eligible.0 {
            quiet()
        } else {
            Color::srgb_u8(161, 66, 32)
        };
    }
}

#[derive(Clone, Copy)]
enum Action {
    Target(usize),
    Saved(usize),
    UseTarget,
    Load,
    Save,
    Apply,
    Restore,
}

fn act(action: Action) -> impl Bundle {
    observe(
        move |_: On<Activate>,
              mut e: ResMut<Editor>,
              mut d: ResMut<Drawing>,
              mut tools: ResMut<Tools>,
              mut looks: ResMut<Looks>| {
            let model = e.model().map(Path::to_owned);
            let key = model
                .as_deref()
                .and_then(|p| p.strip_prefix(&looks.root).ok())
                .map(Path::to_owned);
            e.note.clear();
            match action {
                Action::Target(i) => e.target = i,
                Action::Saved(i) => e.saved = i,
                Action::UseTarget => {
                    d.0 = model
                        .as_deref()
                        .and_then(|p| looks.preview(p))
                        .unwrap_or_else(PixelSkin::template);
                    tools.density = Density::for_skin(&d.0);
                }
                Action::Load => {
                    if let Some(s) = looks.library.designs.get(e.saved) {
                        d.0 = s.clone();
                        tools.density = Density::for_skin(s);
                    }
                }
                Action::Save | Action::Apply => {
                    if !d.0.valid() {
                        e.note = rust_i18n::t!("needTeamPixel", min = MIN_TEAM_PIXELS).into_owned();
                        return;
                    }

                    let bind = matches!(action, Action::Apply);
                    if bind && model.as_ref().is_none_or(|p| !p.is_file()) {
                        e.note = tr("noSkinTarget");
                        return;
                    }

                    match looks.commit(|lib| {
                        let id = lib.save_design(&d.0);
                        if bind && let Some(key) = &key {
                            lib.bind(key, id);
                        }
                        id
                    }) {
                        Ok(id) => {
                            e.saved = id;
                            e.note = tr(if bind { "skinApplied" } else { "skinSaved" });
                        }
                        Err(err) => e.note = format!("{}: {err}", tr("saveError")),
                    }
                }
                Action::Restore => {
                    if let Some(key) = key {
                        match looks.commit(|lib| lib.unbind(&key)) {
                            Ok(()) => e.note = tr("skinRestored"),
                            Err(err) => e.note = format!("{}: {err}", tr("saveError")),
                        }
                    }
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_a_grid_that_preserves_the_loaded_details() {
        let mut skin = PixelSkin::template();
        assert!(Density::for_skin(&skin) == Density::Simple);

        skin.set(0, 0, b'a');
        assert!(Density::for_skin(&skin) == Density::Fine);
    }

    #[test]
    fn fine_strokes_survive_mode_switches_and_simple_strokes_edit_only_one_block() {
        let mut app = App::new();
        app.insert_resource(Drawing(PixelSkin::template()))
            .insert_resource(Tools {
                density: Density::Fine,
                guides: true,
            })
            .insert_resource(Brush(6))
            .init_resource::<ButtonInput<MouseButton>>()
            .add_systems(Update, stroke);
        let pos = app
            .world_mut()
            .spawn((
                Canvas,
                RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::new(
                        1.5 / WIDTH as f32 - 0.5,
                        0.5 / HEIGHT as f32 - 0.5,
                    )),
                },
            ))
            .id();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();

        assert_eq!(&app.world().resource::<Drawing>().0.cells()[..4], b".a..");
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.world_mut().resource_mut::<Tools>().density = Density::Simple;
        app.update();

        assert_eq!(&app.world().resource::<Drawing>().0.cells()[..4], b".a..");
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(pos)
            .unwrap()
            .normalized
            .as_mut()
            .unwrap()
            .x = 3.5 / WIDTH as f32 - 0.5;
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let cells = app.world().resource::<Drawing>().0.cells();
        assert_eq!(&cells[..4], b".aaa");
        assert_eq!(&cells[WIDTH..WIDTH + 4], b"..aa");
        assert_eq!(&cells[2 * WIDTH..2 * WIDTH + 4], b"....");
    }
}
