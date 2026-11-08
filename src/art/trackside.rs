//! Rally-stage trackside dressing (`world::trackside` places it): concrete barriers outside
//! hairpins, spectator tape on stakes and the crowd behind it. Hay bales are props
//! (`art::props`, they can be knocked away).
//!
//! - **Barriers** and **tape** are one shared mesh + material each (instanced), oriented and
//!   scaled from the mesh's own bounds so a re-export doesn't move them.
//! - **The crowd** is impostor cards (`card_crowd`, `tools/art/render_crowd_cards.py`): each
//!   spectator is a camera-facing quad showing the atlas frame for the side the camera sees,
//!   swapped as the view angle changes ([`face_crowd`]).

use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use engine::prelude::AssetsDir;
use engine::render_budget::Scenery;

use super::pbr;
use crate::world::{BARRIER_LEN, StageWorld, TAPE_LEN, TrackKind};

/// Barriers and tape fade out by this distance (m); the crowd a little sooner.
const TRACK_END: f32 = 260.0;
const CROWD_END: f32 = 180.0;

pub struct TracksidePlugin;

impl Plugin for TracksidePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, face_crowd.before(bevy::transform::TransformSystems::Propagate));
    }
}

/// A mesh file with its material, and the transform that maps it onto a piece of `len` m
/// along +Z (origin at the base, centred).
struct Kind {
    parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>)>,
    fit: Transform,
}

/// Loads `names` (one material each) and fits them, as one piece, to `len` m along +Z.
fn kind(world: &mut World, names: &[(&str, &str)], len: f32, f: impl Fn(&mut StandardMaterial) + Copy) -> Option<Kind> {
    let dir = world.resource::<AssetsDir>().0.clone();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    let mut parts = Vec::new();
    for (mesh, set) in names {
        let m = match super::meshfile::load(&dir, mesh) {
            Ok(m) => m,
            Err(e) => {
                error!("trackside: mesh {mesh}: {e}");
                return None;
            }
        };
        if let Some(a) = m.get_aabb() {
            lo = lo.min(a.min().into());
            hi = hi.max(a.max().into());
        }
        let mat = pbr(world, set, f);
        parts.push((world.resource_mut::<Assets<Mesh>>().add(m), mat));
    }
    let size = hi - lo;
    // The long side goes along +Z; stretch that side only to `len` (a few % at most).
    let along_x = size.x > size.z;
    let long = if along_x { size.x } else { size.z };
    let stretch = if long > 0.1 { len / long } else { 1.0 };
    let rot = if along_x { Quat::from_rotation_y(std::f32::consts::FRAC_PI_2) } else { Quat::IDENTITY };
    // Centre it in plan (the base stays where the mesh has it).
    let centre = Vec3::new((lo.x + hi.x) * 0.5, 0.0, (lo.z + hi.z) * 0.5);
    let fit = Transform { rotation: rot, scale: if along_x { Vec3::new(stretch, 1.0, 1.0) } else { Vec3::new(1.0, 1.0, stretch) }, ..default() } * Transform::from_translation(-centre);
    info!("trackside: {:?} {:.2} x {:.2} x {:.2} m, stretched {stretch:.2} to {len} m", names.iter().map(|n| n.0).collect::<Vec<_>>(), size.x, size.y, size.z);
    Some(Kind { parts, fit })
}

/// Spawns the stage's barriers, tape and crowd (visual runs only).
pub fn spawn(world: &mut World, s: &StageWorld) {
    if s.trackside.is_empty() {
        return;
    }
    let wet = world.get_resource::<super::Wet>().is_some_and(|w| w.0);
    let rough = move |m: &mut StandardMaterial| {
        if wet {
            m.perceptual_roughness *= 0.6;
        }
    };
    let barrier = kind(world, &[("barrier_concrete", "m_barrier_concrete")], BARRIER_LEN, rough);
    let tape = kind(world, &[("tape_run", "m_tape"), ("tape_stakes", "m_stake")], TAPE_LEN, move |m| {
        rough(m);
        m.double_sided = true;
        m.cull_mode = None;
    });
    let mut crowd = Crowd::load(world);
    let track_range = VisibilityRange { start_margin: 0.0..0.0, end_margin: (TRACK_END - 30.0)..TRACK_END, use_aabb: false };
    let crowd_range = VisibilityRange { start_margin: 0.0..0.0, end_margin: (CROWD_END - 25.0)..CROWD_END, use_aabb: false };
    let (mut n_barrier, mut n_tape, mut n_crowd) = (0, 0, 0);
    for t in s.trackside.iter() {
        let place = Transform::from_translation(t.pos).with_rotation(Quat::from_rotation_y(t.yaw));
        match t.kind {
            TrackKind::Barrier | TrackKind::Tape => {
                let Some(k) = (if t.kind == TrackKind::Barrier { barrier.as_ref() } else { tape.as_ref() }) else { continue };
                let tf = place * k.fit;
                for (m, mat) in &k.parts {
                    let mut e = world.spawn((Name::new("trackside"), Scenery, tf, Mesh3d(m.clone()), MeshMaterial3d(mat.clone()), track_range.clone()));
                    if t.kind == TrackKind::Tape {
                        e.insert(NotShadowCaster);
                    }
                }
                if t.kind == TrackKind::Barrier {
                    n_barrier += 1;
                } else {
                    n_tape += 1;
                }
            }
            TrackKind::Crowd => {
                let Some(c) = crowd.as_mut() else { continue };
                let person = (t.seed % c.people as u64) as usize;
                // Cards are to scale; people vary a little in size.
                let scale = 0.94 + ((t.seed >> 20) % 100) as f32 * 0.0012;
                let tint = c.tint(t.seed);
                let mat = c.material(world, tint);
                world.spawn((
                    Name::new("spectator"),
                    Spectator { person, yaw: t.yaw, frame: usize::MAX },
                    Mesh3d(c.frames[person * c.angles].clone()),
                    MeshMaterial3d(mat),
                    Transform::from_translation(t.pos).with_scale(Vec3::splat(scale)),
                    NotShadowCaster,
                    crowd_range.clone(),
                ));
                n_crowd += 1;
            }
        }
    }
    if let Some(c) = crowd {
        world.insert_resource(c.frames_res());
    }
    info!("trackside: {n_barrier} barriers, {n_tape} tape runs, {n_crowd} spectators");
}

/// One spectator card: which person, which way they face (yaw of +Z), the frame shown.
#[derive(Component)]
pub struct Spectator {
    person: usize,
    yaw: f32,
    frame: usize,
}

/// The crowd atlas: one quad mesh per (person, angle) frame, to scale, feet at y = 0.
#[derive(Resource, Clone)]
struct CrowdFrames {
    frames: Vec<Handle<Mesh>>,
    angles: usize,
    clockwise: bool,
}

struct Crowd {
    frames: Vec<Handle<Mesh>>,
    people: usize,
    angles: usize,
    clockwise: bool,
    base: StandardMaterial,
    /// Shared materials by tint index.
    mats: Vec<Option<Handle<StandardMaterial>>>,
}

/// A few muted clothing tints (multiplied over the card: darker/warmer/cooler variants of the
/// same people, so ten people read as a crowd).
const TINTS: [[f32; 3]; 4] = [[1.0, 1.0, 1.0], [0.82, 0.86, 0.95], [0.95, 0.88, 0.8], [0.78, 0.78, 0.78]];

impl Crowd {
    /// Reads `textures/card_crowd/frame.txt` (`tools/art/render_crowd_cards.py`). Two layouts:
    /// - **fixed scale**: `atlas W H`, `cell CW CH` (px), `grid COLS ROWS` (row = person,
    ///   column = angle), `mpp` (metres per px, every cell), `foot` (px below the ground
    ///   line); the camera a × 360/COLS degrees counter-clockwise (from above) from the
    ///   person's front.
    /// - **per person**: `people P`, `angles A`, `cols C`, `rows R`, `aspect` (cell w / h),
    ///   `height i H` (m, fills the cell; feet on its bottom edge); cell p * A + a row-major;
    ///   the camera a × 360/A degrees clockwise. Square atlas.
    fn load(world: &mut World) -> Option<Crowd> {
        let dir = world.resource::<AssetsDir>().0.clone();
        let txt = std::fs::read_to_string(dir.join("textures/card_crowd/frame.txt")).map_err(|e| error!("trackside: card_crowd/frame.txt: {e}")).ok()?;
        let mut kv: std::collections::HashMap<String, Vec<f32>> = std::collections::HashMap::new();
        let mut heights = Vec::new();
        for line in txt.lines().filter(|l| !l.trim_start().starts_with('#')) {
            let w: Vec<&str> = line.split_whitespace().collect();
            match w.as_slice() {
                ["height", i, v] => {
                    let i: usize = i.parse().ok()?;
                    if heights.len() <= i {
                        heights.resize(i + 1, 1.25);
                    }
                    heights[i] = v.parse().ok()?;
                }
                [k, rest @ ..] if rest.len() <= 2 && rest.iter().all(|v| v.parse::<f32>().is_ok()) => {
                    kv.insert(k.to_string(), rest.iter().map(|v| v.parse().unwrap()).collect());
                }
                _ => {}
            }
        }
        let get = |k: &str, i: usize| kv.get(k).and_then(|v| v.get(i)).copied();
        // Per frame: uv rect, quad width, height and how far the quad's bottom is below ground.
        let mut rects: Vec<([f32; 4], f32, f32, f32)> = Vec::new();
        let (angles, people, clockwise);
        if let (Some(aw), Some(ah), Some(cw), Some(ch), Some(gc), Some(gr), Some(mpp)) = (get("atlas", 0), get("atlas", 1), get("cell", 0), get("cell", 1), get("grid", 0), get("grid", 1), get("mpp", 0)) {
            let foot = get("foot", 0).unwrap_or(0.0);
            (angles, people, clockwise) = (gc as usize, gr as usize, false);
            for r in 0..people {
                for a in 0..angles {
                    let (u0, v0) = (a as f32 * cw / aw, r as f32 * ch / ah);
                    rects.push(([u0, v0, u0 + cw / aw, v0 + ch / ah], cw * mpp, ch * mpp, foot * mpp));
                }
            }
        } else if let (Some(p), Some(a), Some(c), Some(asp)) = (get("people", 0), get("angles", 0), get("cols", 0), get("aspect", 0)) {
            (angles, people, clockwise) = (a as usize, p as usize, true);
            let (du, dv) = (1.0 / c, 1.0 / (c * asp));
            for k in 0..people * angles {
                let h = heights.get(k / angles).copied().unwrap_or(1.25);
                let (u0, v0) = ((k % c as usize) as f32 * du, (k / c as usize) as f32 * dv);
                rects.push(([u0, v0, u0 + du, v0 + dv], h * asp, h, 0.0));
            }
        } else {
            error!("trackside: card_crowd/frame.txt: unknown layout");
            return None;
        }
        if angles == 0 || people == 0 || rects.iter().any(|r| r.0[2] > 1.0001 || r.0[3] > 1.0001) {
            error!("trackside: card_crowd/frame.txt: bad layout");
            return None;
        }
        let mut meshes = world.resource_mut::<Assets<Mesh>>();
        let mut frames = Vec::with_capacity(rects.len());
        for ([u0, v0, u1, v1], w, h, below) in rects {
            let mut m = Mesh::new(bevy::mesh::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default());
            m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[-w * 0.5, -below, 0.0], [w * 0.5, -below, 0.0], [w * 0.5, h - below, 0.0], [-w * 0.5, h - below, 0.0]]);
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4]);
            m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[u0, v1], [u1, v1], [u1, v0], [u0, v0]]);
            m.insert_indices(bevy::mesh::Indices::U16(vec![0, 1, 2, 0, 2, 3]));
            frames.push(meshes.add(m));
        }
        let base = {
            let h = pbr(world, "card_crowd", |m| {
                m.alpha_mode = AlphaMode::Mask(0.5);
                m.double_sided = true;
                m.cull_mode = None;
                m.perceptual_roughness = 0.85;
            });
            world.resource::<Assets<StandardMaterial>>().get(&h).cloned()?
        };
        Some(Crowd { frames, people, angles, clockwise, base, mats: vec![None; TINTS.len()] })
    }

    fn tint(&self, seed: u64) -> usize {
        ((seed >> 8) % TINTS.len() as u64) as usize
    }

    fn material(&mut self, world: &mut World, tint: usize) -> Handle<StandardMaterial> {
        if let Some(h) = &self.mats[tint] {
            return h.clone();
        }
        let mut m = self.base.clone();
        let t = TINTS[tint];
        m.base_color = Color::srgb(t[0], t[1], t[2]);
        let h = world.resource_mut::<Assets<StandardMaterial>>().add(m);
        self.mats[tint] = Some(h.clone());
        h
    }

    fn frames_res(&self) -> CrowdFrames {
        CrowdFrames { frames: self.frames.clone(), angles: self.angles, clockwise: self.clockwise }
    }
}

/// Turns each spectator card to face the camera (about Y) and shows the frame for the side of
/// the person the camera sees.
fn face_crowd(frames: Option<Res<CrowdFrames>>, cams: Query<&GlobalTransform, With<engine::gfx::MainCamera>>, mut q: Query<(&mut Spectator, &mut Transform, &mut Mesh3d)>) {
    let (Some(frames), Ok(cam)) = (frames, cams.single()) else { return };
    let eye = cam.translation();
    for (mut s, mut tf, mut mesh) in &mut q {
        let to_cam = eye - tf.translation;
        if to_cam.x * to_cam.x + to_cam.z * to_cam.z < 1e-4 {
            continue;
        }
        let view_yaw = to_cam.x.atan2(to_cam.z);
        tf.rotation = Quat::from_rotation_y(view_yaw);
        // Angle 0 = seen from the front. A positive turn about +Y is counter-clockwise from
        // above.
        let turn = if frames.clockwise { s.yaw - view_yaw } else { view_yaw - s.yaw };
        let rel = turn.rem_euclid(std::f32::consts::TAU);
        let a = ((rel / std::f32::consts::TAU * frames.angles as f32).round() as usize) % frames.angles;
        let f = s.person * frames.angles + a;
        if f != s.frame {
            s.frame = f;
            if let Some(h) = frames.frames.get(f) {
                mesh.0 = h.clone();
            }
        }
    }
}
