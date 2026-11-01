//! The co-driver's tablet: a pace-note screen held on the knees.
//!
//! - **Screen**: the guide's map and notes (`map.rs`, `panel.rs`) render into [`TabletImage`]
//!   (a [`TABLET_PX`] texture) through the map camera, heading-up around the car's live
//!   position, with the call-timing cue. Nothing is drawn twice: the in-car tablet is the
//!   Map screen re-hosted.
//! - **Object**: a dark slab with that image on its face (unlit, so it reads at night),
//!   resting on the guide's knees in the car's frame. Look down to read it; look up for the
//!   road.
//! - **Raise**: hold the raise key (Space, right mouse, left trigger) and the tablet comes up in
//!   front of the eyes, filling most of the view, eased over [`RAISE_S`]; let go and it goes
//!   back down.
//! - **Hands**: the guide's gloved hands hold its sides, arms bent from the shoulders (the
//!   driver arms' two-bone IK, `art::hands::elbow`).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use engine::actions::{ActionDef, ActionState, AddAction, Binding};
use engine::render_budget::Actor;

use super::map::TabletImage;
use super::seat::{GuideCar, Ride, SeatCamera, SeatSystems, capsule_between, in_car};
use super::GuideView;

/// The screen's texture size (px): 4:3, like a pace-note tablet.
pub const TABLET_PX: UVec2 = UVec2::new(1200, 900);
/// The slab (m): an 11-inch tablet with its bezel; the screen inside it.
pub const BODY: Vec3 = Vec3::new(0.262, 0.2, 0.009);
pub const SCREEN: Vec2 = Vec2::new(0.24, 0.18);
/// Raise / lower time (s).
pub const RAISE_S: f32 = 0.22;
/// Share of the view's height the raised tablet fills.
pub const RAISED_FILL: f32 = 0.84;

/// The guide's right shoulder from the eye (car frame; mirrored for the left).
const SHOULDER: Vec3 = Vec3::new(0.2, -0.27, 0.16);

/// Where the hands hold its sides (tablet frame, m below the centre): low, clear of the text.
const GRIP_Y: f32 = -0.06;

/// The tablet in the scene.
#[derive(Component)]
pub struct Tablet;

/// How far raised (0 on the knees … 1 in front of the eyes), eased.
#[derive(Resource, Default)]
pub struct Raise {
    pub target: f32,
    pub t: f32,
}

#[derive(Component)]
struct GuideArm {
    side: usize,
    upper: Entity,
    fore: Entity,
}

pub(super) fn build(app: &mut App) {
    app.init_resource::<Raise>()
        .add_action(
            ActionDef::button("tablet_raise")
                .label("Raise the tablet (hold)")
                .group("Guide")
                .bind(Binding::key(KeyCode::Space))
                .bind(Binding::mouse(MouseButton::Right))
                .bind(Binding::pad(bevy::input::gamepad::GamepadButton::LeftTrigger2)),
        )
        .add_action(ActionDef::button("guide_call").label("Call the note (call timing)").group("Guide").bind(Binding::key(KeyCode::KeyE)).bind(Binding::pad(bevy::input::gamepad::GamepadButton::South)))
        .add_systems(Update, ensure_image.run_if(not(resource_exists::<TabletImage>)))
        .add_systems(Update, (spawn.run_if(no_tablet), hold).chain().after(SeatSystems).run_if(in_car));
}

fn ensure_image(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_target_texture(TABLET_PX.x, TABLET_PX.y, TextureFormat::Rgba8UnormSrgb, None);
    image.sampler = ImageSampler::linear();
    image.asset_usage = RenderAssetUsages::RENDER_WORLD;
    commands.insert_resource(TabletImage(images.add(image)));
}

fn no_tablet(q: Query<(), With<Tablet>>, cams: Query<(), With<SeatCamera>>) -> bool {
    q.is_empty() && !cams.is_empty()
}

fn spawn(mut commands: Commands, image: Res<TabletImage>, mut meshes: ResMut<Assets<Mesh>>, mut mats: ResMut<Assets<StandardMaterial>>) {
    let body = meshes.add(Cuboid::new(BODY.x, BODY.y, BODY.z));
    let screen = meshes.add(Rectangle::new(SCREEN.x, SCREEN.y));
    let shell = mats.add(StandardMaterial { base_color: Color::srgb(0.035, 0.037, 0.04), perceptual_roughness: 0.45, metallic: 0.2, ..default() });
    // Unlit and a touch brighter than the texture: a lit display, readable in shade and at night.
    let face = mats.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: Some(image.0.clone()), unlit: true, ..default() });
    let glove = mats.add(StandardMaterial { base_color: Color::srgb(0.12, 0.12, 0.13), perceptual_roughness: 0.75, ..default() });
    let suit = mats.add(StandardMaterial { base_color: super::seat::CODRIVER_SUIT, perceptual_roughness: 0.85, ..default() });
    let palm = meshes.add(Sphere::new(1.0).mesh().uv(14, 9));
    let thumb = meshes.add(Capsule3d::new(0.012, 0.045));
    let fore = meshes.add(Capsule3d::new(0.034, 0.27));
    let upper = meshes.add(Capsule3d::new(0.042, 0.28));
    let tablet = commands
        .spawn((Name::new("co-driver tablet"), GuideView, Tablet, Transform::default(), Visibility::default(), Actor))
        .with_children(|t| {
            t.spawn((Mesh3d(body), MeshMaterial3d(shell), Transform::from_xyz(0.0, 0.0, -BODY.z * 0.5), Actor));
            t.spawn((Mesh3d(screen), MeshMaterial3d(face), Transform::from_xyz(0.0, 0.0, 0.0005), Actor));
            // Hands on its sides: palms behind, thumbs over the bezel.
            for s in [-1.0f32, 1.0] {
                let x = s * (BODY.x * 0.5 - 0.004);
                t.spawn((Mesh3d(palm.clone()), MeshMaterial3d(glove.clone()), Transform::from_xyz(x + s * 0.01, GRIP_Y, -0.018).with_scale(Vec3::new(0.022, 0.042, 0.016)), Actor));
                let th = Transform::from_xyz(x - s * 0.004, GRIP_Y + 0.012, 0.008).with_rotation(Quat::from_rotation_z(s * 0.5));
                t.spawn((Mesh3d(thumb.clone()), MeshMaterial3d(glove.clone()), th, Actor));
            }
        })
        .id();
    for side in 0..2 {
        let u = commands.spawn((Name::new("co-driver upper arm"), GuideView, Mesh3d(upper.clone()), MeshMaterial3d(suit.clone()), Transform::default(), Actor)).id();
        let f = commands.spawn((Name::new("co-driver forearm"), GuideView, Mesh3d(fore.clone()), MeshMaterial3d(suit.clone()), Transform::default(), Actor)).id();
        commands.spawn((GuideView, GuideArm { side, upper: u, fore: f }));
    }
    let _ = tablet;
}

/// Where the tablet rests on the knees, in the car's frame, for a co-driver eye at `eye`:
/// held on the thighs below the eye, its face tilted up to the eye.
pub fn lap_pose(eye: Vec3) -> Transform {
    let p = eye + Vec3::new(0.0, -0.42, -0.3);
    // Face (+Z) towards the eye, top edge towards the front of the car.
    let to_eye = (eye - p).normalize();
    Transform::from_translation(p).looking_to(-to_eye, Vec3::NEG_Z)
}

/// Where it's held up to read, in the camera's frame, for a vertical field of view `fov`.
pub fn raised_pose(fov: f32) -> Transform {
    let d = BODY.y / (2.0 * (fov * 0.5).tan() * RAISED_FILL);
    Transform::from_xyz(0.0, -0.01, -d.max(0.12))
}

fn ease(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Raises / lowers the tablet and poses it and the arms holding it.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn hold(
    time: Res<Time>,
    actions: Option<Res<ActionState>>,
    ride: Res<Ride>,
    mut raise: ResMut<Raise>,
    cars: Query<&Transform, (With<GuideCar>, Without<Tablet>, Without<SeatCamera>)>,
    cams: Query<(&Transform, &Projection), (With<SeatCamera>, Without<Tablet>)>,
    mut tablets: Query<&mut Transform, With<Tablet>>,
    arms: Query<&GuideArm>,
    mut parts: Query<&mut Transform, (Without<Tablet>, Without<GuideCar>, Without<SeatCamera>)>,
) {
    let (Ok(car), Ok((cam, proj)), Ok(mut tab)) = (cars.single(), cams.single(), tablets.single_mut()) else { return };
    raise.target = if actions.is_some_and(|a| a.pressed("tablet_raise")) { 1.0 } else { 0.0 };
    let step = time.delta_secs() / RAISE_S;
    raise.t = if raise.target > raise.t { (raise.t + step).min(raise.target) } else { (raise.t - step).max(raise.target) };
    let k = ease(raise.t);
    let eye = ride.eye.unwrap_or(Vec3::new(0.37, 0.6, 0.5));
    let lap = car.mul_transform(lap_pose(eye));
    let fov = match proj {
        Projection::Perspective(p) => p.fov,
        _ => 1.4,
    };
    let up = cam.mul_transform(raised_pose(fov));
    tab.translation = lap.translation.lerp(up.translation, k);
    tab.rotation = lap.rotation.slerp(up.rotation, k);
    // Arms: shoulders in the car's frame, hands on the tablet's sides.
    for a in &arms {
        let s = if a.side == 0 { -1.0 } else { 1.0 };
        let shoulder = car.transform_point(eye + SHOULDER * Vec3::new(s, 1.0, 1.0));
        let hand = tab.transform_point(Vec3::new(s * (BODY.x * 0.5 + 0.01), GRIP_Y - 0.01, -0.03));
        let wrist = hand + (shoulder - hand).normalize_or_zero() * 0.06;
        let pole = car.rotation * Vec3::new(s * 0.9, -0.8, 0.3);
        let elbow = crate::art::hands::elbow(shoulder, wrist, pole);
        if let Ok(mut t) = parts.get_mut(a.upper) {
            *t = capsule_between(shoulder, elbow, 0.28 + 0.084);
        }
        if let Ok(mut t) = parts.get_mut(a.fore) {
            *t = capsule_between(elbow, wrist, 0.27 + 0.068);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tablet_rests_below_the_eye_facing_it() {
        let eye = Vec3::new(0.37, 0.8, 0.6);
        let t = lap_pose(eye);
        assert!(t.translation.y < eye.y - 0.4 && t.translation.z < eye.z);
        let face = t.rotation * Vec3::Z;
        assert!(face.dot((eye - t.translation).normalize()) > 0.99, "{face}");
        // Its top edge points to the front of the car.
        assert!((t.rotation * Vec3::Y).z < -0.3);
    }

    #[test]
    fn raised_it_fills_most_of_the_view() {
        let fov = 80f32.to_radians();
        let t = raised_pose(fov);
        let d = -t.translation.z;
        let visible = 2.0 * d * (fov * 0.5).tan();
        assert!((BODY.y / visible - RAISED_FILL).abs() < 0.02 || d <= 0.1201);
        // Facing the camera.
        assert!((t.rotation * Vec3::Z).dot(Vec3::Z) > 0.999);
    }
}
