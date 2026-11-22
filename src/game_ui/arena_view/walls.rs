use super::*;
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology},
};

/// 同一层的所有墙为同色实心几何。L/T 交会处重叠而不描边，不会出现看似可穿过的接缝。
fn surface(map: &ArenaMap, bevel: f32) -> Mesh {
    let mut points = Vec::<[f32; 3]>::new();
    let mut indices = Vec::new();
    for wall in map.wall_rectangles() {
        let a = xy(wall.min, map);
        let b = xy(wall.max, map);
        let lo = a.min(b) + Vec2::Y * bevel;
        let hi = a.max(b);

        let start = points.len() as u32;
        for corner in [hi, Vec2::new(lo.x, hi.y), lo, Vec2::new(hi.x, lo.y)] {
            points.push(corner.extend(0.).to_array());
        }

        indices.extend([start, start + 1, start + 2, start, start + 2, start + 3]);
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, points)
    .with_inserted_indices(Indices::U32(indices))
}

pub(super) fn spawn(
    p: &mut ChildSpawnerCommands,
    map: &ArenaMap,
    meshes: &mut Assets<Mesh>,
    palette: &Palette,
) {
    for (inset, color, z) in [
        (0., &palette.wall_edge, 1.),
        (tile(map) * 0.016, &palette.wall_face, 1.05),
    ] {
        p.spawn((
            Mesh2d(meshes.add(surface(map, inset))),
            MeshMaterial2d(color.clone()),
            Transform::from_xyz(0., 0., z),
        ));
    }
}
