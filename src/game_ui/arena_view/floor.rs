use super::*;
use bevy::{
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::VertexAttributeValues,
};

pub(super) fn material(
    server: &AssetServer,
    materials: &mut Assets<ColorMaterial>,
) -> Handle<ColorMaterial> {
    let mut repeat = ImageSamplerDescriptor::linear();
    repeat.set_address_mode(ImageAddressMode::Repeat);
    let tile = artwork::texture(server, artwork::FLOOR, ImageSampler::Descriptor(repeat));
    materials.add(ColorMaterial::from(tile))
}

pub(super) fn spawn(
    p: &mut ChildSpawnerCommands,
    map: &ArenaMap,
    meshes: &mut Assets<Mesh>,
    palette: &Palette,
) {
    let (w, h) = map.size();
    let cells = Vec2::new(w as f32, h as f32);
    let size = cells * tile(map);
    let mut floor = Mesh::from(Rectangle::new(size.x, size.y));

    // 一格平铺一次；无论迷宫大小只使用一个四边形，不为地板标线创建实体。
    if let Some(VertexAttributeValues::Float32x2(uv)) = floor.attribute_mut(Mesh::ATTRIBUTE_UV_0) {
        for point in uv {
            *point = (Vec2::from_array(*point) * cells).to_array();
        }
    }
    p.spawn((
        Mesh2d(meshes.add(floor)),
        MeshMaterial2d(palette.floor.clone()),
    ));
}
