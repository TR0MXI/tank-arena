mod editor;
mod store;

use super::*;
pub(super) use store::Looks;
use tank_arena::appearance::{HEIGHT, MIN_TEAM_PIXELS, PALETTE, PixelSkin, SCALE, WIDTH, rgba};

pub(super) fn install(app: &mut App) {
    app.init_resource::<Looks>();
    editor::install(app);
}
