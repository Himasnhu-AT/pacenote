//! The mirrors from the cockpit: a small camera at the rear-view mirror looks back and renders
//! into a low-resolution image (linear, pre-exposed, like the reflection probes), which the
//! rear-view mirror's glass and the two door mirrors show unlit, so the main camera's tone
//! mapping treats it like the rest of the scene. Only in the cockpit view at High quality and
//! above; elsewhere (and for the chase camera) the camera is off and the door mirrors show their
//! dark glass.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Exposure, RenderTarget};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;

use crate::drive::CameraView;

/// Size of the mirror image (px): the rear-view mirror is ~4:1.
pub const SIZE: UVec2 = UVec2::new(384, 96);
/// Horizontal field of the rear view (rad).
const FOV_X: f32 = 0.85;

/// Rear-view mirror glass in the model frame (`cockpit.py`: under the windscreen header).
const REAR_MIRROR: Vec3 = Vec3::new(0.0, 0.985, 0.02);

/// How much brighter the mirror is exposed than the main camera (EV): it has no sun.
const MIRROR_EV_LIFT: f32 = 1.3;

/// Render layer of what the mirror shows (besides layer 0, which it doesn't see).
pub const MIRROR_LAYER: usize = 1;

/// Render layers of something both the main camera and the mirror draw.
pub fn seen() -> RenderLayers {
    RenderLayers::from_layers(&[0, MIRROR_LAYER])
}

/// The mirror camera (child of the car model).
#[derive(Component)]
pub struct MirrorCamera;

/// The image the mirrors show.
#[derive(Resource, Clone)]
pub struct MirrorImage {
    pub image: Handle<Image>,
    /// Unlit material showing it (door mirrors).
    pub door: Handle<StandardMaterial>,
}

/// Turns the mirror camera off (dev, for an A/B of its cost:
/// `ecs.res.set {"name": "pacenote::art::mirror::MirrorSwitch", "value": {"off": true}}`).
#[derive(Resource, Reflect, Default)]
#[reflect(Resource)]
pub struct MirrorSwitch {
    pub off: bool,
}

/// A door mirror's picture (hidden when the mirror camera is off).
#[derive(Component)]
pub struct DoorMirrorPicture;

pub struct MirrorPlugin;

impl Plugin for MirrorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MirrorSwitch>().register_type::<MirrorSwitch>().add_systems(Update, (follow_main, switch, tag).chain());
    }
}

/// What the mirror shows besides what is spawned with [`seen`]: the terrain and the road.
/// It sees no sun (the sun is on layer 0 only), so it renders no shadow maps of its own:
/// the cost is a few hundred draws in a narrow view.
fn tag(mut commands: Commands, chunks: Query<Entity, (Added<engine::world::TerrainChunk>, Without<RenderLayers>)>, roads: Query<Entity, Added<crate::open::live::LiveRoadMesh>>, children: Query<&Children>, meshes: Query<(), (With<Mesh3d>, Without<RenderLayers>)>) {
    for e in &chunks {
        commands.entity(e).insert(seen());
    }
    for r in &roads {
        for e in std::iter::once(r).chain(children.iter_descendants(r)) {
            if meshes.contains(e) {
                commands.entity(e).insert(seen());
            }
        }
    }
}

/// Spawns the mirror camera under the car model and makes the shared image. Visual worlds only.
pub fn spawn(world: &mut World, model: Entity) {
    let image = match world.get_resource::<MirrorImage>() {
        Some(m) => m.image.clone(),
        None => {
            let mut img = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba16Float, None);
            img.sampler = bevy::image::ImageSampler::linear();
            let image = world.resource_mut::<Assets<Image>>().add(img);
            let door = world.resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { base_color_texture: Some(image.clone()), unlit: true, ..default() });
            world.insert_resource(MirrorImage { image: image.clone(), door });
            image
        }
    };
    let aspect = SIZE.x as f32 / SIZE.y as f32;
    let fov_y = 2.0 * ((FOV_X * 0.5).tan() / aspect).atan();
    let cam = world
        .spawn((
            Name::new("mirror camera"),
            MirrorCamera,
            Camera3d::default(),
            Camera { order: -1, is_active: false, clear_color: ClearColorConfig::Custom(Color::srgb(0.6, 0.66, 0.74)), ..default() },
            RenderTarget::from(image),
            Projection::Perspective(PerspectiveProjection { fov: fov_y, aspect_ratio: aspect, near: 0.05, far: 900.0, ..default() }),
            // Linear, pre-exposed radiance: the main camera tone-maps it with the scene.
            Tonemapping::None,
            DebandDither::Disabled,
            bevy::camera::Hdr,
            Msaa::Off,
            Exposure::default(),
            RenderLayers::layer(MIRROR_LAYER),
            // Looking back (the model's +Z), from just behind the glass.
            Transform::from_translation(REAR_MIRROR + Vec3::new(0.0, 0.01, 0.03)).looking_to(Vec3::Z, Vec3::Y),
        ))
        .id();
    world.entity_mut(model).add_child(cam);
}

/// Shows the mirror image on the rear-view mirror's glass (the model's `Mirror` material:
/// unlit, flipped left to right as a mirror is) and on picture quads over the door mirrors'
/// glass (children of `MirrorL` / `MirrorR`). Called once when the car's rig is found.
pub fn dress(commands: &mut Commands, mats: &mut Assets<StandardMaterial>, meshes: &mut Assets<Mesh>, rear: Option<Handle<StandardMaterial>>, doors: [Option<Entity>; 2], mirror: Option<&MirrorImage>) {
    let Some(mirror) = mirror else { return };
    if let Some(h) = rear
        && let Some(mut m) = mats.get_mut(&h)
    {
        m.base_color = Color::WHITE;
        m.base_color_texture = Some(mirror.image.clone());
        m.unlit = true;
        m.uv_transform = bevy::math::Affine2::from_scale_angle_translation(Vec2::new(-1.0, 1.0), 0.0, Vec2::new(1.0, 0.0));
    }
    // Door mirror glass (`build_rallycar.py` `mirror_mesh`, node frame): faces back (+Z) at
    // z 0.053, x −0.113..0.048 (left; the right is mirrored), y −0.051..0.044. Each shows the
    // outer part of the rear view, flipped.
    for (i, door) in doors.iter().enumerate() {
        let Some(door) = *door else { continue };
        let s = if i == 0 { 1.0 } else { -1.0 };
        let (x0, x1) = (-0.111 * s, 0.046 * s);
        let (y0, y1, z) = (-0.049, 0.042, 0.0545);
        // Image x of the outer and inner edges (the rear camera sees the car's left on its right).
        let (u_out, u_in) = if i == 0 { (1.0, 0.62) } else { (0.0, 0.38) };
        let (v0, v1) = (0.62, 0.2);
        let mesh = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::RENDER_WORLD)
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[x0, y0, z], [x1, y0, z], [x1, y1, z], [x0, y1, z]])
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[u_out, v0], [u_in, v0], [u_in, v1], [u_out, v1]])
            .with_inserted_indices(bevy::mesh::Indices::U32(if s > 0.0 { vec![0, 1, 2, 0, 2, 3] } else { vec![0, 2, 1, 0, 3, 2] }));
        let pic = commands
            .spawn((Name::new("door mirror picture"), DoorMirrorPicture, Mesh3d(meshes.add(mesh)), MeshMaterial3d(mirror.door.clone()), Transform::default(), Visibility::Hidden, bevy::light::NotShadowCaster, engine::render_budget::Actor))
            .id();
        commands.entity(door).add_child(pic);
    }
}

/// The mirror camera follows the main camera's exposure and fog.
fn follow_main(main: Query<(&Exposure, Option<&DistanceFog>), (With<engine::gfx::MainCamera>, Without<MirrorCamera>)>, mut mirror: Query<(Entity, &mut Exposure, Option<&mut DistanceFog>), With<MirrorCamera>>, mut commands: Commands) {
    let Some((exp, fog)) = main.iter().next() else { return };
    for (e, mut x, f) in &mut mirror {
        // No sun in the mirror (see [`tag`]): sky light only, so it is exposed brighter.
        let ev = exp.ev100 - MIRROR_EV_LIFT;
        if x.ev100 != ev {
            x.ev100 = ev;
        }
        match (fog, f) {
            (Some(src), Some(mut dst)) => {
                if dst.color != src.color {
                    *dst = src.clone();
                }
            }
            (Some(src), None) => {
                commands.entity(e).insert(src.clone());
            }
            _ => {}
        }
    }
}

/// On in the cockpit view at High quality or above; the door pictures show while it's on.
fn switch(
    view: Option<Res<CameraView>>,
    settings: Option<Res<engine::settings::CoreSettings>>,
    switch: Res<MirrorSwitch>,
    mut cams: Query<&mut Camera, With<MirrorCamera>>,
    mut pics: Query<&mut Visibility, With<DoorMirrorPicture>>,
) {
    let q = settings.map_or(2, |c| c.quality.index());
    let on = q >= 2 && view.is_some_and(|v| *v == CameraView::Cockpit) && !switch.off;
    for mut c in &mut cams {
        if c.is_active != on {
            c.is_active = on;
        }
    }
    for mut v in &mut pics {
        v.set_if_neq(if on { Visibility::Inherited } else { Visibility::Hidden });
    }
}
