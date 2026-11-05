//! Race liveries and bodies: rivals' and the player's traffic-shape bodies get a rally look
//! (a white roof plate with the race number, side stripes, door plates), and the player's car
//! shows its own body shape in the chase view (the cockpit and bonnet views keep the rally
//! hatch's interior).
//!
//! **Owner: stream F (GAMEPLAY).** Cheap: one shared unit-cube mesh and three shared
//! materials, scaled boxes per piece (the number is a 7-segment figure of boxes), so Bevy
//! batches them. Visual worlds only.

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use engine::flow::AppState;

use super::garage::{self, Body, Garage, Paint};
use crate::drive::{CameraView, CarBody};

/// Shared meshes and materials for liveries.
#[derive(Resource, Clone)]
pub struct LiveryArt {
    cube: Handle<Mesh>,
    white: Handle<StandardMaterial>,
    black: Handle<StandardMaterial>,
    /// Stripe colour per paint (contrasting).
    stripe: [Handle<StandardMaterial>; 2],
    /// Race paints per colour (solid, metallic): stronger than the traffic's, which read as
    /// near white under the valley's light.
    paint: Vec<[Handle<StandardMaterial>; 2]>,
}

/// Race paint colours (sRGB) by `Paint::colour`: white, silver, black, red, blue, green, yellow.
const RACE_PAINT: [[f32; 3]; 7] = [[0.85, 0.85, 0.83], [0.42, 0.44, 0.48], [0.02, 0.022, 0.026], [0.5, 0.012, 0.01], [0.012, 0.05, 0.38], [0.012, 0.2, 0.05], [0.85, 0.45, 0.01]];

impl LiveryArt {
    pub fn new(meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> LiveryArt {
        let m = |c: Color| StandardMaterial { base_color: c, perceptual_roughness: 0.5, ..default() };
        LiveryArt {
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            white: mats.add(m(Color::srgb(0.93, 0.93, 0.9))),
            black: mats.add(m(Color::srgb(0.03, 0.03, 0.035))),
            stripe: [mats.add(m(Color::srgb(0.95, 0.72, 0.08))), mats.add(m(Color::srgb(0.93, 0.93, 0.9)))],
            paint: RACE_PAINT
                .iter()
                .map(|c| {
                    let base = Color::srgb(c[0], c[1], c[2]);
                    [
                        // No clear coat and a low reflectance: the sky's sheen washed the traffic
                        // paints out to near white.
                        mats.add(StandardMaterial { base_color: base, perceptual_roughness: 0.6, reflectance: 0.25, ..default() }),
                        mats.add(StandardMaterial { base_color: base, metallic: 0.35, perceptual_roughness: 0.45, reflectance: 0.3, ..default() }),
                    ]
                })
                .collect(),
        }
    }
}

/// Roof (height, local z of its centre) and the body's half width, per traffic shape (hatch,
/// saloon, estate, SUV; from `art::traffic_car`'s profiles, z flipped: + = rearward).
const ROOF: [(f32, f32, f32, f32); 4] = [(1.42, 0.7, 0.86, 4.05), (1.42, 0.33, 0.88, 4.5), (1.43, 0.9, 0.88, 4.6), (1.72, 0.8, 0.92, 4.45)];

/// Is a paint light (white, silver, yellow): dark stripes and figures read on it.
fn light(p: Paint) -> bool {
    matches!(p.colour, 0 | 1 | 6)
}

/// The 7 segments lit for each digit (a b c d e f g).
const SEGS: [u8; 10] = [0b1111110, 0b0110000, 0b1101101, 0b1111001, 0b0110011, 0b1011011, 0b1011111, 0b1110000, 0b1111111, 0b1111011];

/// Spawns a two-digit figure (`n`) as boxes on a plane: `origin` its centre, `right` and `up`
/// the plane's axes (unit), `normal` out of it; digit height `h`.
#[allow(clippy::too_many_arguments)]
fn figure(p: &mut ChildSpawnerCommands, art: &LiveryArt, mat: &Handle<StandardMaterial>, n: u8, origin: Vec3, right: Vec3, up: Vec3, normal: Vec3, h: f32) {
    let w = h * 0.55;
    let t = h * 0.12;
    let digits: Vec<u8> = if n >= 10 { vec![n / 10, n % 10] } else { vec![n] };
    let count = digits.len() as f32;
    let rot = Quat::from_mat3(&Mat3::from_cols(right, up, normal));
    for (k, d) in digits.iter().enumerate() {
        let cx = (k as f32 - (count - 1.0) * 0.5) * (w + h * 0.25);
        let c = origin + right * cx;
        // (offset x, offset y, horizontal?) for a..g.
        let segs = [(0.0, 0.5, true), (0.5, 0.25, false), (0.5, -0.25, false), (0.0, -0.5, true), (-0.5, -0.25, false), (-0.5, 0.25, false), (0.0, 0.0, true)];
        for (i, (sx, sy, horiz)) in segs.iter().enumerate() {
            if SEGS[*d as usize % 10] & (1 << (6 - i)) == 0 {
                continue;
            }
            let pos = c + right * (sx * w) + up * (sy * h);
            let scale = if *horiz { Vec3::new(w, t, 0.01) } else { Vec3::new(t, h * 0.5, 0.01) };
            p.spawn((Mesh3d(art.cube.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(pos).with_rotation(rot).with_scale(scale), NotShadowCaster));
        }
    }
}

/// Spawns the livery for a traffic-shape body under its root (origin on the ground, −Z
/// forward): roof plate and number, a stripe down each side, a plate with the number on each
/// door.
pub fn spawn_livery(p: &mut ChildSpawnerCommands, art: &LiveryArt, shape: usize, paint: Paint, number: u8) {
    let (roof_y, roof_z, half_w, len) = ROOF[shape.min(3)];
    let stripe = &art.stripe[light(paint) as usize ^ 1];
    // Roof plate, white, and the number on it (reads from behind: its top towards the nose).
    let plate = Vec3::new(0.0, roof_y + 0.012, roof_z);
    p.spawn((Mesh3d(art.cube.clone()), MeshMaterial3d(art.white.clone()), Transform::from_translation(plate).with_scale(Vec3::new(0.74, 0.02, 0.74)), NotShadowCaster));
    figure(p, art, &art.black, number, plate + Vec3::Y * 0.012, Vec3::X, Vec3::NEG_Z, Vec3::Y, 0.42);
    for side in [-1.0f32, 1.0] {
        // Stripe along the flank.
        p.spawn((Mesh3d(art.cube.clone()), MeshMaterial3d(stripe.clone()), Transform::from_xyz(side * (half_w + 0.012), 0.62, 0.0).with_scale(Vec3::new(0.02, 0.1, len * 0.86)), NotShadowCaster));
        // Door plate with the number, readable from the side.
        let door = Vec3::new(side * (half_w + 0.02), 0.82, 0.05);
        p.spawn((Mesh3d(art.cube.clone()), MeshMaterial3d(art.white.clone()), Transform::from_translation(door).with_scale(Vec3::new(0.02, 0.34, 0.5)), NotShadowCaster));
        // Read from outside: "right" runs to the tail on the left flank, to the nose on the
        // right one.
        let right = if side < 0.0 { Vec3::Z } else { Vec3::NEG_Z };
        figure(p, art, &art.black, number, door + Vec3::X * side * 0.012, right, Vec3::Y, Vec3::X * side, 0.24);
    }
}

/// A traffic-shape body whose first child (the painted shell) still has the traffic paint.
#[derive(Component)]
pub struct NeedsRacePaint(pub Paint);

/// Puts the race paint on the shell once the body's children exist.
fn race_paint(mut commands: Commands, art: Option<Res<LiveryArt>>, bodies: Query<(Entity, &NeedsRacePaint, &Children)>) {
    let Some(art) = art else { return };
    for (e, p, kids) in &bodies {
        let Some(shell) = kids.first() else { continue };
        let m = art.paint[(p.0.colour % garage::PAINTS) as usize][p.0.metallic as usize].clone();
        commands.entity(*shell).insert(MeshMaterial3d(m));
        commands.entity(e).remove::<NeedsRacePaint>();
    }
}

/// The player's car body in the chase view (non-rally cars).
#[derive(Component)]
pub struct PlayerBody;

/// The shown player body's key (model, paint, number), to rebuild on a change.
#[derive(Component, PartialEq, Eq, Clone)]
struct PlayerBodyKey(String, Paint, u8);

pub struct LiveryPlugin;

impl Plugin for LiveryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (player_body, show_player_body, race_paint).chain().run_if(resource_exists::<crate::world::StageWorld>));
    }
}

/// Makes sure the shared art exists (returns false the first frame it is made).
pub fn ensure_art(commands: &mut Commands, art: &Option<Res<LiveryArt>>, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>) -> bool {
    if art.is_none() {
        commands.insert_resource(LiveryArt::new(meshes, mats));
        return false;
    }
    true
}

/// Spawns (or respawns) the traffic-shape body of the active car under the driven car.
#[allow(clippy::too_many_arguments)]
fn player_body(
    mut commands: Commands,
    garage: Res<Garage>,
    cars: Query<Entity, With<CarBody>>,
    bodies: Query<(Entity, &PlayerBodyKey, &ChildOf), With<PlayerBody>>,
    art: Option<Res<LiveryArt>>,
    traffic: Option<Res<crate::art::traffic_car::TrafficArt>>,
    meshes: Option<ResMut<Assets<Mesh>>>,
    mats: Option<ResMut<Assets<StandardMaterial>>>,
) {
    let (Some(mut meshes), Some(mut mats)) = (meshes, mats) else { return };
    let Ok(car) = cars.single() else { return };
    let active = garage.active_car();
    let def = active.def().unwrap_or(&garage::CARS[0]);
    let want = (def.body != Body::Rally).then(|| PlayerBodyKey(def.id.to_string(), active.paint, active.number));
    let have = bodies.iter().find(|b| b.2.parent() == car);
    if have.map(|b| b.1) == want.as_ref() {
        return;
    }
    for (e, _, _) in &bodies {
        commands.entity(e).despawn();
    }
    let Some(want) = want else { return };
    if !ensure_art(&mut commands, &art, &mut meshes, &mut mats) {
        return;
    }
    let Some(traffic) = traffic else {
        commands.insert_resource(crate::art::traffic_car::TrafficArt::new(&mut meshes, &mut mats));
        return;
    };
    let art = art.unwrap();
    let shape = super::race_flow::body_shape(def.body);
    let id = super::race_flow::art_id(shape, active.paint.metallic);
    let paint = active.paint;
    let body = commands
        .spawn((Name::new("player body"), PlayerBody, NeedsRacePaint(paint), want, Transform::from_xyz(0.0, -0.47, 0.0), Visibility::Hidden, DespawnOnExit(AppState::InGame)))
        .with_children(|p| {
            crate::art::traffic_car::spawn_parts(p, &traffic, id, paint.colour);
            spawn_livery(p, &art, shape, paint, active.number);
        })
        .id();
    commands.entity(car).add_child(body);
}

/// Chase view: the car's own body; cockpit and bonnet: the rally hatch (its interior).
fn show_player_body(view: Option<Res<CameraView>>, mut bodies: Query<&mut Visibility, (With<PlayerBody>, Without<crate::art::car::CarModel>)>, mut models: Query<(&mut Visibility, &ChildOf), With<crate::art::car::CarModel>>, cars: Query<Entity, With<CarBody>>) {
    let chase = view.is_some_and(|v| *v == CameraView::Chase);
    let has_body = !bodies.is_empty();
    for mut v in &mut bodies {
        v.set_if_neq(if chase { Visibility::Inherited } else { Visibility::Hidden });
    }
    let Ok(car) = cars.single() else { return };
    for (mut v, parent) in &mut models {
        if parent.parent() != car {
            continue;
        }
        v.set_if_neq(if chase && has_body { Visibility::Hidden } else { Visibility::Inherited });
    }
}
