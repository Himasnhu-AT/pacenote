//! A fixed free-roam drive for measuring (docs/ASSETS_BRIEF.md §1): from the village along the
//! valley B-road, then up the basin gravel road. The
//! dev autopilot follows `StageWorld::road`, so `open.tour` puts this route there (traffic stays
//! on: it's free roam, not an event) and the car at its start.

use std::sync::Arc;

use bevy::prelude::*;

use super::roads::{RoadKind, RoadNet};
use crate::stage::Road;

/// The tour's centreline, from the network (deterministic): the valley B-road from the
/// village east to its end, then up the basin gravel road.
pub fn route(net: &RoadNet) -> Option<Road> {
    let valley = &net.roads.first()?.road;
    let basin = net.roads.iter().find(|r| r.kind == RoadKind::Gravel)?;
    let from = valley.nearest(net.places().village, 400.0)?.i;
    let (mut pts, mut y, mut w, mut s) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut take = |r: &Road, idx: &mut dyn Iterator<Item = usize>| {
        for i in idx {
            let p = r.pts[i];
            if pts.last().is_some_and(|q: &Vec2| q.distance(Vec2::new(p.x, p.z)) < 1.0) {
                continue;
            }
            pts.push(Vec2::new(p.x, p.z));
            y.push(p.y);
            w.push(r.width[i]);
            s.push(r.surface[i]);
        }
    };
    take(valley, &mut (from..valley.len()));
    take(&basin.road, &mut (0..basin.road.len().min(800)));
    Some(Road::from_centreline(&pts, &y, w, s))
}

/// Puts the tour on the stage's road and the car at its start (dev: measuring, videos).
pub fn start(world: &mut World) -> Result<f32, String> {
    let open = world.get_resource::<crate::world::StageWorld>().and_then(|s| s.open.clone()).ok_or("not in the open world")?;
    let road = route(&open.net).ok_or("no tour route")?;
    let len = road.length();
    // Traffic still drives and is drawn (it's part of the cost being measured), but isn't solid.
    open.traffic_solid.store(false, std::sync::atomic::Ordering::Relaxed);
    let start = crate::meta::ghost::start_car(&road);
    {
        // The autopilot plans its speed from the route's corners.
        let mut st = world.resource_mut::<crate::world::StageWorld>();
        st.corners = Arc::new(crate::stage::corners(&road));
        st.crests = Arc::new(crate::stage::crests(&road));
        st.road = Arc::new(road);
    }
    let mut q = world.query::<&mut crate::drive::DrivenCar>();
    for mut c in q.iter_mut(world) {
        c.car = start.clone();
        c.prev = (start.pos, start.rot);
    }
    Ok(len)
}

#[cfg(test)]
mod tests {
    #[test]
    fn tour_goes_from_town_to_the_ring() {
        let net = super::super::roads::generate(super::super::SEED);
        let r = super::route(&net).expect("route");
        assert!(r.length() > 2000.0, "{} m", r.length());
        for w in r.pts.windows(2) {
            assert!(w[0].distance(w[1]) < 4.5, "gap in the route at {} -> {} ({} m)", w[0], w[1], w[0].distance(w[1]));
        }
    }
}
