//! Driver + guide over the network (milestone 2).
//!
//! - [`sync`]: what replicates from the driver (stage id + road hash, the run, the car) and
//!   the driver's publishing systems.
//! - This file: the guide's side of the data: it regenerates the stage from the id
//!   ([`GuideStage`]) and keeps the latest car / run ([`GuideFeed`]), with the clock
//!   extrapolated between updates. The guide never runs the car simulation: it has no
//!   `StageWorld` and no `DrivenCar`, so the drive systems and the HUD never run there.
//! - [`map`] + [`panel`]: the guide's screen (top-down map, corner list, clock, splits).
//! - [`notice`]: the driver's small "guide connected / disconnected" label.
//!
//! Session and roles: `crate::net_session`.

pub mod map;
pub mod notice;
pub mod panel;
pub mod sync;

use std::sync::Arc;

use bevy::prelude::*;
use engine::net::Remote;

use crate::net_session::{NetSessionPlugin, Role, Session};
use crate::stage::{self, Corner, Road, StageDef};
use sync::{NetCar, NetRun, NetStage};

/// The stage as the guide knows it: generated from the driver's stage id.
#[derive(Resource, Clone)]
pub struct GuideStage {
    pub def: StageDef,
    pub road: Arc<Road>,
    pub corners: Arc<Vec<Corner>>,
    /// Crests on straights (metres along the road).
    pub crests: Arc<Vec<f32>>,
    /// Our road hash, and whether it matches the driver's.
    pub road_hash: u64,
    pub hash_ok: bool,
}

impl GuideStage {
    /// Generates stage `id` (the road and pace data only: no terrain, no dressing).
    pub fn generate(id: u32) -> Option<GuideStage> {
        let def = stage::stages().into_iter().find(|d| d.id == id)?;
        let road = stage::generate_road(&def);
        let corners = stage::corners(&road);
        let crests = stage::crests(&road);
        let road_hash = sync::road_hash(&road);
        Some(GuideStage { def, road: Arc::new(road), corners: Arc::new(corners), crests: Arc::new(crests), road_hash, hash_ok: true })
    }
}

/// The latest from the driver, for the guide's views. Kept when the link drops.
#[derive(Resource, Clone, Default)]
pub struct GuideFeed {
    /// Interpolated car (None until the first snapshot).
    pub car: Option<NetCar>,
    pub run: NetRun,
    /// The driver's stage id and road hash (0 = the driver is in a menu).
    pub stage: NetStage,
    /// Metres along the road where the car is now.
    pub s: f32,
    /// Stage clock to show (time + penalty), extrapolated between run updates.
    pub clock: f32,
    run_at: f64,
}

/// Run condition: this machine is the guide.
pub fn is_guide(role: Res<Role>) -> bool {
    *role == Role::Guide
}

/// Run condition: show the guide's screen (the guide, in game).
pub fn view_wanted(role: Res<Role>, state: Option<Res<State<engine::flow::AppState>>>) -> bool {
    *role == Role::Guide && state.is_some_and(|s| *s.get() == engine::flow::AppState::InGame)
}

/// Everything on the guide's screen (despawned when it isn't wanted any more).
#[derive(Component)]
pub struct GuideView;

/// Run condition: this machine is the driver (hosting).
pub fn is_driver(role: Res<Role>) -> bool {
    *role == Role::Driver
}

pub struct GuidePlugin;

impl Plugin for GuidePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NetSessionPlugin).init_resource::<GuideFeed>();
        sync::register(app);
        app.add_systems(Update, (follow_stage, update_feed, enter_game).chain().run_if(is_guide));
        app.add_systems(Update, despawn_view.run_if(not(view_wanted)));
        let visuals = app.world().get_resource::<engine::flow::Headless>().is_none_or(|h| !h.0);
        if visuals {
            map::build(app);
            panel::build(app);
            notice::build(app);
        }
    }
}

/// A guide that joined from the menus goes in when the driver's stage is known.
fn enter_game(stage: Option<Res<GuideStage>>, state: Option<Res<State<engine::flow::AppState>>>, mut next: ResMut<NextState<engine::flow::AppState>>) {
    if stage.is_some() && state.is_some_and(|s| *s.get() == engine::flow::AppState::MainMenu) {
        next.set(engine::flow::AppState::InGame);
    }
}

fn despawn_view(mut commands: Commands, q: Query<Entity, With<GuideView>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

/// Regenerate the stage when the driver's stage id changes.
fn follow_stage(mut commands: Commands, q: Query<&NetStage, With<Remote>>, current: Option<ResMut<GuideStage>>) {
    let Some(ns) = q.iter().next().copied() else { return };
    if ns.id == 0 {
        return;
    }
    match current {
        Some(mut g) if g.def.id == ns.id => {
            let ok = g.road_hash == ns.road_hash;
            if g.hash_ok != ok {
                g.hash_ok = ok;
            }
        }
        _ => {
            let t0 = std::time::Instant::now();
            let Some(mut g) = GuideStage::generate(ns.id) else {
                error!("guide: the driver is on stage {} which this build doesn't have", ns.id);
                return;
            };
            g.hash_ok = g.road_hash == ns.road_hash;
            info!(
                "guide: stage {} \"{}\" generated in {:.0} ms, {} corners, road hash {:016x} ({})",
                g.def.id,
                g.def.name,
                t0.elapsed().as_secs_f64() * 1000.0,
                g.corners.len(),
                g.road_hash,
                if g.hash_ok { "matches the driver" } else { "DIFFERS from the driver" }
            );
            commands.insert_resource(g);
        }
    }
}

fn update_feed(
    time: Res<Time<Real>>,
    session: Res<Session>,
    stage: Option<Res<GuideStage>>,
    cars: Query<&NetCar, With<Remote>>,
    runs: Query<(Ref<NetRun>, &NetStage), With<Remote>>,
    mut feed: ResMut<GuideFeed>,
) {
    let now = time.elapsed_secs_f64();
    if let Some(c) = cars.iter().next() {
        feed.car = Some(*c);
    }
    if let Some((run, ns)) = runs.iter().next() {
        if run.is_changed() || feed.run != *run {
            feed.run = *run;
            feed.run_at = now;
        }
        feed.stage = *ns;
    }
    let r = feed.run;
    let ahead = if r.running() && session.connected { (now - feed.run_at).min(0.5) as f32 } else { 0.0 };
    feed.clock = r.finished.unwrap_or(r.time + r.penalty + ahead);
    // Where the car is along the road: its nearest station, unless that jumps far from the
    // driver's progress (the road passing close to itself).
    let s = match (&stage, feed.car) {
        (Some(g), Some(c)) => g.road.nearest(Vec2::new(c.pos.x, c.pos.z), 60.0).map(|rp| rp.s).filter(|s| (s - r.progress).abs() < 150.0).unwrap_or(r.progress),
        _ => r.progress,
    };
    if feed.s != s {
        feed.s = s;
    }
}

/// One line of the guide's notes: a corner or a crest on a straight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Note {
    /// Metres along the road where it starts.
    pub at: f32,
    pub end: f32,
    /// `Some(corner)` index into `GuideStage::corners`, `None` = a crest on a straight.
    pub corner: Option<usize>,
    /// Follows the previous note closely (< 30 m): "into".
    pub into: bool,
}

/// The next `out.capacity()` notes from `s` (corners still in progress included).
pub fn upcoming(g: &GuideStage, s: f32, max: usize, out: &mut Vec<Note>) {
    upcoming_in(&g.corners, &g.crests, s, max, out);
}

/// [`upcoming`] over any stage's corners and crests (the AI co-driver reads the driver's own).
pub fn upcoming_in(corners: &[Corner], crests: &[f32], s: f32, max: usize, out: &mut Vec<Note>) {
    out.clear();
    let first = corners.partition_point(|c| c.end < s);
    let mut ci = first;
    let mut ki = crests.partition_point(|&k| k < s);
    let mut prev_end = if first > 0 { corners[first - 1].end } else { -1e9 };
    while out.len() < max {
        let c = corners.get(ci);
        // Crests that fall inside a corner are part of its call already.
        while let Some(&k) = crests.get(ki) {
            if corners.iter().skip(ci.saturating_sub(1)).take(3).any(|c| k >= c.start - 30.0 && k <= c.end) {
                ki += 1;
            } else {
                break;
            }
        }
        let k = crests.get(ki).copied();
        let note = match (c, k) {
            (Some(c), Some(k)) if k < c.start => {
                ki += 1;
                Note { at: k, end: k, corner: None, into: false }
            }
            (Some(c), _) => {
                ci += 1;
                Note { at: c.start, end: c.end, corner: Some(ci - 1), into: c.start - prev_end < 30.0 }
            }
            (None, Some(k)) => {
                ki += 1;
                Note { at: k, end: k, corner: None, into: false }
            }
            (None, None) => break,
        };
        if note.corner.is_some() {
            prev_end = note.end;
        }
        out.push(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upcoming_notes_are_in_order_and_start_at_the_car() {
        let g = GuideStage::generate(1).unwrap();
        let mut v = Vec::with_capacity(8);
        upcoming(&g, 0.0, 8, &mut v);
        assert_eq!(v.len(), 8);
        assert!(v.windows(2).all(|w| w[0].at <= w[1].at));
        let mid = g.road.length() * 0.5;
        upcoming(&g, mid, 8, &mut v);
        assert!(v.iter().all(|n| n.end >= mid));
        upcoming(&g, g.road.length() + 1.0, 8, &mut v);
        assert!(v.is_empty());
    }
}
