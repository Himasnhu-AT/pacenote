//! What the menus know about each stage without loading it: the road (generated once in a
//! background thread at startup and cached), its hash, length, corner count, and two
//! pictures drawn from it on the CPU: a small top-down road preview for the stage list and a
//! large contour map for the menu backdrop.
//!
//! [`stage_hash`] identifies the generated road: leaderboard records and ghosts carry it, so
//! entries driven before a generator change are flagged instead of compared.

use std::sync::{Arc, Mutex};

use bevy::asset::RenderAssetUsages;
use bevy::math::{Vec2, Vec3};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::stage::{self, Road, StageDef, Surface, TimeOfDay, Weather};

use super::board::Hasher;

/// Hash of a generated stage: its definition and every road station (position, width,
/// banking, surface). Hex, 16 digits.
pub fn stage_hash(def: &StageDef, road: &Road) -> String {
    let mut h = Hasher::default();
    h.u64(def.id as u64);
    h.u64(def.seed);
    h.f32(def.length);
    h.u64(road.len() as u64);
    for i in 0..road.len() {
        let p = road.pts[i];
        h.f32(p.x);
        h.f32(p.y);
        h.f32(p.z);
        h.f32(road.width[i]);
        h.f32(road.bank[i]);
        h.u64(road.surface[i] as u64);
    }
    format!("{:016x}", h.0)
}

/// Menu facts about one stage.
#[derive(Clone, Debug)]
pub struct StageInfo {
    pub def: StageDef,
    pub hash: String,
    /// Generated road length (m).
    pub length: f32,
    pub corners: usize,
    /// Hairpins and grade-2 corners.
    pub tight: usize,
    pub crests: usize,
    /// Road (XZ) for drawing, every 8 m.
    pub line: Vec<Vec2>,
    /// Share of the road per surface (for "gravel 82%, mud 18%").
    pub surfaces: Vec<(Surface, f32)>,
    /// Climb total (m).
    pub climb: f32,
}

impl StageInfo {
    pub fn build(def: &StageDef) -> StageInfo {
        let road = stage::generate_road(def);
        Self::from_road(def, &road)
    }
    pub fn from_road(def: &StageDef, road: &Road) -> StageInfo {
        let corners = stage::corners(road);
        let tight = corners.iter().filter(|c| c.grade <= 2).count();
        let crests = stage::crests(road).len();
        let line = road.pts.iter().step_by(4).map(|p| Vec2::new(p.x, p.z)).chain(road.pts.last().map(|p| Vec2::new(p.x, p.z))).collect();
        let mut counts: Vec<(Surface, f32)> = Vec::new();
        for s in &road.surface {
            match counts.iter_mut().find(|(k, _)| k == s) {
                Some(c) => c.1 += 1.0,
                None => counts.push((*s, 1.0)),
            }
        }
        let n = road.surface.len().max(1) as f32;
        for c in &mut counts {
            c.1 /= n;
        }
        counts.sort_by(|a, b| b.1.total_cmp(&a.1));
        let climb = road.pts.windows(2).map(|w| (w[1].y - w[0].y).max(0.0)).sum();
        StageInfo { def: def.clone(), hash: stage_hash(def, road), length: road.length(), corners: corners.len(), tight, crests, line, surfaces: counts, climb }
    }
}

/// An RGBA8 picture.
#[derive(Clone, Debug)]
pub struct Raster {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

impl Raster {
    pub fn new(w: u32, h: u32) -> Raster {
        Raster { w, h, px: vec![0; (w * h * 4) as usize] }
    }
    /// Alpha-blends `c` (straight alpha) at pixel (x, y) with coverage `a`.
    fn blend(&mut self, x: i32, y: i32, c: [f32; 4], a: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || a <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let sa = (c[3] * a).clamp(0.0, 1.0);
        let da = self.px[i + 3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        for (k, ck) in c.iter().take(3).enumerate() {
            let d = self.px[i + k] as f32 / 255.0;
            let o = if oa > 0.0 { (ck * sa + d * da * (1.0 - sa)) / oa } else { 0.0 };
            self.px[i + k] = (o * 255.0).round() as u8;
        }
        self.px[i + 3] = (oa * 255.0).round() as u8;
    }
    /// An anti-aliased thick segment.
    pub fn line(&mut self, a: Vec2, b: Vec2, width: f32, c: [f32; 4]) {
        let r = width * 0.5;
        let (x0, x1) = ((a.x.min(b.x) - r - 1.0).floor() as i32, (a.x.max(b.x) + r + 1.0).ceil() as i32);
        let (y0, y1) = ((a.y.min(b.y) - r - 1.0).floor() as i32, (a.y.max(b.y) + r + 1.0).ceil() as i32);
        let ab = b - a;
        let l2 = ab.length_squared().max(1e-6);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
                let d = p.distance(a + ab * t);
                let cov = (r - d + 0.5).clamp(0.0, 1.0);
                self.max_blend(x, y, c, cov);
            }
        }
    }
    /// Like `blend` but a polyline drawn as segments doesn't double up at the joints: keeps
    /// the higher coverage for pixels already painted in this colour.
    fn max_blend(&mut self, x: i32, y: i32, c: [f32; 4], cov: f32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 || cov <= 0.0 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        let want = (c[3] * cov * 255.0).round() as u8;
        let same = (0..3).all(|k| (self.px[i + k] as i32 - (c[k] * 255.0).round() as i32).abs() <= 2);
        if same && self.px[i + 3] >= want {
            return;
        }
        if same {
            self.px[i + 3] = want;
        } else {
            self.blend(x, y, c, cov);
        }
    }
    pub fn polyline(&mut self, pts: &[Vec2], width: f32, c: [f32; 4]) {
        for w in pts.windows(2) {
            self.line(w[0], w[1], width, c);
        }
    }
    pub fn dot(&mut self, at: Vec2, radius: f32, c: [f32; 4]) {
        let (x0, x1) = ((at.x - radius - 1.0).floor() as i32, (at.x + radius + 1.0).ceil() as i32);
        let (y0, y1) = ((at.y - radius - 1.0).floor() as i32, (at.y + radius + 1.0).ceil() as i32);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(at);
                self.blend(x, y, c, (radius - d + 0.5).clamp(0.0, 1.0));
            }
        }
    }
    pub fn image(&self) -> Image {
        Image::new(
            Extent3d { width: self.w, height: self.h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            self.px.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
        )
    }
}

/// Maps road XZ into a `size` px square with `pad` px margin (x right, z down, like the map).
fn fit(line: &[Vec2], size: f32, pad: f32) -> impl Fn(Vec2) -> Vec2 + use<> {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in line {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    let span = (hi - lo).max_element().max(1.0);
    let scale = (size - 2.0 * pad) / span;
    let off = (Vec2::splat(size) - (hi - lo) * scale) * 0.5;
    move |p: Vec2| off + (p - lo) * scale
}

pub const PAPER: [f32; 4] = [0.95, 0.93, 0.88, 1.0];
pub const START: [f32; 4] = [0.37, 0.88, 0.54, 1.0];
pub const FINISH: [f32; 4] = [1.0, 0.36, 0.29, 1.0];

/// The small stage-list preview: the road, start (green) and finish (red).
pub fn preview(info: &StageInfo, size: u32) -> Raster {
    let mut r = Raster::new(size, size);
    let s = size as f32;
    let f = fit(&info.line, s, s * 0.08);
    let pts: Vec<Vec2> = info.line.iter().map(|p| f(*p)).collect();
    let w = (s / 90.0).max(2.0);
    r.polyline(&pts, w * 2.6, [0.0, 0.0, 0.0, 0.35]);
    r.polyline(&pts, w, PAPER);
    if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
        r.dot(*a, w * 2.4, START);
        r.dot(*b, w * 2.4, FINISH);
    }
    r
}

/// The menu backdrop: terrain contours of the whole stage map with the road on top.
/// `ink` is the contour colour (alpha = strength of the index contours).
pub fn contour_map(def: &StageDef, road: &Road, size: u32, ink: [f32; 3]) -> Raster {
    let mut r = contours(def, size, ink, 1.0);
    let half = stage::half_size(def);
    let s = size as f32;
    // The road, in map space (same square as the contours).
    let to_px = |p: Vec3| Vec2::new((p.x + half) / (2.0 * half) * s, (p.z + half) / (2.0 * half) * s);
    let pts: Vec<Vec2> = road.pts.iter().step_by(3).map(|p| to_px(*p)).collect();
    let w = (s / 260.0).max(2.0);
    r.polyline(&pts, w * 3.0, [0.03, 0.05, 0.04, 0.55]);
    r.polyline(&pts, w, PAPER);
    if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
        r.dot(*a, w * 2.8, START);
        r.dot(*b, w * 2.8, FINISH);
    }
    r
}

/// The main menu's backdrop: terrain contours of no stage in particular (no road), in `ink`.
pub fn neutral_contours(size: u32, ink: [f32; 3]) -> Raster {
    let mut def = stage::stages().into_iter().next().expect("stages");
    def.seed = 0x5eed_c0de_2026;
    def.biome = stage::Biome::BirchHills;
    contours(&def, size, ink, 0.55)
}

/// Terrain contour lines of a stage's map square; `strength` scales the line alpha.
fn contours(def: &StageDef, size: u32, ink: [f32; 3], strength: f32) -> Raster {
    let half = stage::half_size(def);
    let n = 160usize;
    let cell = 2.0 * half / (n - 1) as f32;
    let mut hgt = vec![0.0f32; n * n];
    for z in 0..n {
        for x in 0..n {
            hgt[z * n + x] = stage::base_height(-half + x as f32 * cell, -half + z as f32 * cell, def);
        }
    }
    let sample = |u: f32, v: f32| {
        let (fx, fz) = ((u * (n - 1) as f32).clamp(0.0, (n - 1) as f32 - 0.001), (v * (n - 1) as f32).clamp(0.0, (n - 1) as f32 - 0.001));
        let (x, z) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - x as f32, fz - z as f32);
        let a = hgt[z * n + x] + (hgt[z * n + x + 1] - hgt[z * n + x]) * tx;
        let b = hgt[(z + 1) * n + x] + (hgt[(z + 1) * n + x + 1] - hgt[(z + 1) * n + x]) * tx;
        a + (b - a) * tz
    };
    let mut r = Raster::new(size, size);
    let s = size as f32;
    let step = 8.0;
    // Contours: where the band index changes between a pixel and its neighbours, with a
    // soft edge from the distance to the iso line (|h - level| / gradient).
    let mut band = vec![0.0f32; (size * size) as usize];
    for y in 0..size {
        for x in 0..size {
            band[(y * size + x) as usize] = sample((x as f32 + 0.5) / s, (y as f32 + 0.5) / s) / step;
        }
    }
    for y in 1..size - 1 {
        for x in 1..size - 1 {
            let i = (y * size + x) as usize;
            let b = band[i];
            let gx = (band[i + 1] - band[i - 1]) * 0.5;
            let gy = (band[i + size as usize] - band[i - size as usize]) * 0.5;
            let g = (gx * gx + gy * gy).sqrt().max(1e-5);
            let d = (b - b.round()).abs() / g;
            let index = (b.round() as i32).rem_euclid(5) == 0;
            let (w, a) = if index { (1.1, 0.55 * strength) } else { (0.6, 0.28 * strength) };
            // Fade out towards the picture's edges (it sits behind the menu, edges unseen).
            let e = ((x.min(size - 1 - x).min(y).min(size - 1 - y)) as f32 / (s * 0.14)).min(1.0);
            let cov = (w - d + 0.5).clamp(0.0, 1.0) * e * e * (3.0 - 2.0 * e);
            if cov > 0.0 {
                r.blend(x as i32, y as i32, [ink[0], ink[1], ink[2], a], cov);
            }
        }
    }
    r
}

/// Size of the stage-list previews (px).
pub const PREVIEW_PX: u32 = 360;
/// Size of the backdrop map (px).
pub const MAP_PX: u32 = 1100;

/// Results of the background generation, filled in as stages finish.
#[derive(Default)]
struct Pending {
    infos: Vec<Option<(StageInfo, Raster)>>,
    map: Option<Raster>,
    backdrop: Option<Raster>,
}

/// Every stage's [`StageInfo`] and preview image, built in a background thread.
#[derive(Resource, Default)]
pub struct StageCatalog {
    pub infos: Vec<Option<StageInfo>>,
    pub previews: Vec<Option<Handle<Image>>>,
    /// Stage 1's contour map with its road (the loading screen's picture for stage 1).
    pub map: Option<Handle<Image>>,
    /// The main menu's backdrop: contours of no stage, no road.
    pub backdrop: Option<Handle<Image>>,
    pending: Option<Arc<Mutex<Pending>>>,
    /// Seconds the background generation took (for the docs / logs).
    pub build_secs: Option<f32>,
}

impl StageCatalog {
    pub fn info(&self, id: u32) -> Option<&StageInfo> {
        self.infos.get(id.saturating_sub(1) as usize).and_then(|i| i.as_ref())
    }
    pub fn preview(&self, id: u32) -> Option<Handle<Image>> {
        self.previews.get(id.saturating_sub(1) as usize).cloned().flatten()
    }
    pub fn ready(&self) -> bool {
        !self.infos.is_empty() && self.infos.iter().all(|i| i.is_some())
    }
    /// Starts the background build (map first, then stages in order).
    pub fn start(&mut self, ink: [f32; 3], backdrop_ink: [f32; 3]) {
        let defs = stage::stages();
        self.infos = vec![None; defs.len()];
        self.previews = vec![None; defs.len()];
        let pending = Arc::new(Mutex::new(Pending { infos: vec![None; defs.len()], map: None, backdrop: None }));
        self.pending = Some(pending.clone());
        let spawn = std::thread::Builder::new().name("stage-catalog".into()).spawn(move || {
            let t0 = std::time::Instant::now();
            let backdrop = neutral_contours(MAP_PX, backdrop_ink);
            pending.lock().unwrap().backdrop = Some(backdrop);
            for (k, def) in defs.iter().enumerate() {
                let road = stage::generate_road(def);
                let info = StageInfo::from_road(def, &road);
                let pic = preview(&info, PREVIEW_PX);
                if k == 0 {
                    let map = contour_map(def, &road, MAP_PX, ink);
                    pending.lock().unwrap().map = Some(map);
                }
                pending.lock().unwrap().infos[k] = Some((info, pic));
            }
            info!("stage catalog: {} stages in {:.2} s", defs.len(), t0.elapsed().as_secs_f32());
        });
        if let Err(e) = spawn {
            warn!("stage catalog thread: {e}");
        }
    }
    /// Moves finished stages into the catalog (and their pictures into `images`, when there
    /// is a renderer). Returns true when something arrived.
    pub fn poll(&mut self, images: Option<&mut Assets<Image>>) -> bool {
        let Some(p) = self.pending.clone() else { return false };
        let mut p = p.lock().unwrap();
        let mut got = false;
        let mut images = images;
        if let Some(b) = p.backdrop.take() {
            if let Some(imgs) = images.as_deref_mut() {
                self.backdrop = Some(imgs.add(b.image()));
            }
            got = true;
        }
        if let Some(map) = p.map.take() {
            if let Some(imgs) = images.as_deref_mut() {
                self.map = Some(imgs.add(map.image()));
            }
            got = true;
        }
        for (k, slot) in p.infos.iter_mut().enumerate() {
            if let Some((info, pic)) = slot.take() {
                if let Some(imgs) = images.as_deref_mut() {
                    self.previews[k] = Some(imgs.add(pic.image()));
                }
                self.infos[k] = Some(info);
                got = true;
            }
        }
        if self.ready() {
            self.pending = None;
        }
        got
    }
    /// Blocks until everything is built (tests, headless tools).
    pub fn wait(&mut self) {
        while !self.ready() {
            self.poll(None);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

// ---------------------------------------------------------------- conditions

/// Weathers the picker offers, in order.
pub const WEATHERS: [Weather; 4] = [Weather::Clear, Weather::Overcast, Weather::Fog, Weather::Rain];
/// Times of day the picker offers, in order.
pub const TIMES: [TimeOfDay; 4] = [TimeOfDay::Morning, TimeOfDay::Noon, TimeOfDay::Afternoon, TimeOfDay::Dusk];

/// The weathers a stage can be driven in: all of them, except rain on the snow stages (rain
/// falling on snow-covered ground reads wrong).
pub fn weathers_for(def: &StageDef) -> Vec<Weather> {
    WEATHERS.into_iter().filter(|w| !(*w == Weather::Rain && def.biome == stage::Biome::SnowForest)).collect()
}

/// `def` with this weather and time of day (the road and terrain don't depend on either).
pub fn with_conditions(mut def: StageDef, weather: Weather, time: TimeOfDay) -> StageDef {
    def.weather = weather;
    def.time = time;
    def
}

/// The published stage `id` as defined (its default weather and time).
pub fn base_def(id: u32) -> Option<StageDef> {
    if id == crate::open::ID {
        return Some(crate::open::def());
    }
    stage::stages().into_iter().find(|d| d.id == id)
}

/// Short key of non-default conditions ("rain-dusk"), `None` at the stage's defaults.
pub fn conditions_key(def: &StageDef) -> Option<String> {
    let base = base_def(def.id)?;
    if base.weather == def.weather && base.time == def.time {
        return None;
    }
    Some(format!("{:?}-{:?}", def.weather, def.time).to_lowercase())
}

/// Driving assists a run is driven with (Settings > Driving). Both on by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assists {
    pub abs: bool,
    pub traction_control: bool,
}

impl Default for Assists {
    fn default() -> Self {
        Assists { abs: true, traction_control: true }
    }
}

impl Assists {
    /// "noabs" / "notc" parts for the board key (none with both on).
    fn key_parts(self) -> impl Iterator<Item = &'static str> {
        [(!self.abs).then_some("noabs"), (!self.traction_control).then_some("notc")].into_iter().flatten()
    }
    /// "ABS off, traction control off" (empty with both on).
    pub fn label(self) -> String {
        let mut v = Vec::new();
        if !self.traction_control {
            v.push("traction control off");
        }
        if !self.abs {
            v.push("ABS off");
        }
        v.join(", ")
    }
}

/// The hash records and ghosts are filed under: the road's hash, plus `@<key>` when the weather
/// or time isn't the stage's default or an assist is off ("rain-dusk", "noabs",
/// "fog-noon-notc"). So each weather, time and assist setting has its own board and ghosts, and
/// times at the defaults keep the hash they always had.
pub fn board_hash(road_hash: &str, def: &StageDef, assists: Assists) -> String {
    let parts: Vec<String> = conditions_key(def).into_iter().chain(assists.key_parts().map(String::from)).collect();
    if parts.is_empty() { road_hash.to_string() } else { format!("{road_hash}@{}", parts.join("-")) }
}

/// Weather / time index in [`WEATHERS`] / [`TIMES`] (for the wire).
pub fn weather_index(w: Weather) -> u8 {
    WEATHERS.iter().position(|x| *x == w).unwrap_or(0) as u8
}
pub fn time_index(t: TimeOfDay) -> u8 {
    TIMES.iter().position(|x| *x == t).unwrap_or(0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conditions_file_separately_and_defaults_keep_their_hash() {
        let def = base_def(3).unwrap();
        assert_eq!(board_hash("abc", &def, Assists::default()), "abc", "defaults: the hash records always had");
        assert_eq!(conditions_key(&def), None);
        let wet = with_conditions(def.clone(), Weather::Rain, TimeOfDay::Dusk);
        assert_eq!(board_hash("abc", &wet, Assists::default()), "abc@rain-dusk");
        assert_ne!(board_hash("abc", &wet, Assists::default()), board_hash("abc", &with_conditions(def.clone(), Weather::Fog, TimeOfDay::Dusk), Assists::default()));
        let off = Assists { abs: false, traction_control: true };
        assert_eq!(board_hash("abc", &def, off), "abc@noabs", "an assist off: its own board");
        assert_eq!(board_hash("abc", &wet, Assists { abs: false, traction_control: false }), "abc@rain-dusk-noabs-notc");
        assert_eq!(stage::generate_road(&wet).pts, stage::generate_road(&def).pts, "weather never changes the road");
        let snow = stage::stages().into_iter().find(|d| d.biome == stage::Biome::SnowForest).unwrap();
        assert!(!weathers_for(&snow).contains(&Weather::Rain));
        assert_eq!(weathers_for(&def).len(), 4);
    }

    #[test]
    fn hash_is_stable_and_tracks_the_road() {
        let def = &stage::stages()[2];
        let road = stage::generate_road(def);
        let a = stage_hash(def, &road);
        assert_eq!(a, stage_hash(def, &stage::generate_road(def)), "same seed → same hash");
        let mut moved = road.clone();
        moved.pts[10].x += 0.01;
        assert_ne!(a, stage_hash(def, &moved), "any road change shows");
        let other = &stage::stages()[3];
        assert_ne!(a, stage_hash(other, &stage::generate_road(other)));
    }

    #[test]
    fn info_and_pictures() {
        let def = &stage::stages()[0];
        let t0 = std::time::Instant::now();
        let road = stage::generate_road(def);
        let gen_secs = t0.elapsed().as_secs_f32();
        let info = StageInfo::from_road(def, &road);
        assert!((info.length - road.length()).abs() < 1e-3);
        assert!(info.corners > 5);
        assert!((info.surfaces.iter().map(|s| s.1).sum::<f32>() - 1.0).abs() < 1e-3);
        let t1 = std::time::Instant::now();
        let pic = preview(&info, 128);
        let map = contour_map(def, &road, 256, [0.8, 0.7, 0.3]);
        eprintln!("road {gen_secs:.3} s, pictures {:.3} s, hash {}", t1.elapsed().as_secs_f32(), info.hash);
        let painted = |r: &Raster| r.px.chunks(4).filter(|p| p[3] > 0).count();
        assert!(painted(&pic) > 300, "the road shows in the preview");
        assert!(painted(&map) > 2000, "contours show on the map");
        assert_eq!(pic.px.len(), 128 * 128 * 4);
    }
}
