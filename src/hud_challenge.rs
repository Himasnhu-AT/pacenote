//! HUD: challenge approach cards, live readouts and result cards (top centre).
//!
//! **Owner: stream F (GAMEPLAY).** Design: `docs/festival/GAME_SYSTEMS.md` §3, `docs/festival/UX_PASS.md` §3.2, `docs/festival/DECISIONS.md` C1.
//! A read-only view of F's state; E owns `hud.rs` and the shared HUD look (`hud::plate`).
//! Despawn on leaving `InGame`; allocate nothing per frame while values are unchanged.

use bevy::prelude::*;

pub struct ChallengeHudPlugin;

impl Plugin for ChallengeHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, icons::make);
    }
}

/// Small HUD glyphs the fonts don't have (★ ☆ ✓ ✕), drawn into images at startup. White with
/// alpha coverage: tint them with `ImageNode::color`.
pub mod icons {
    use bevy::asset::RenderAssetUsages;
    use bevy::prelude::*;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

    const N: u32 = 64;
    /// Supersamples per axis.
    const SS: u32 = 4;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Icon {
        Star,
        StarHollow,
        Check,
        Cross,
    }

    #[derive(Resource, Clone)]
    pub struct HudIcons([Handle<Image>; 4]);

    impl HudIcons {
        pub fn get(&self, i: Icon) -> Handle<Image> {
            self.0[i as usize].clone()
        }
    }

    pub(super) fn make(mut commands: Commands, images: Option<ResMut<Assets<Image>>>) {
        let Some(mut images) = images else { return };
        let mut img = |i: Icon| images.add(image(&raster(i)));
        commands.insert_resource(HudIcons([img(Icon::Star), img(Icon::StarHollow), img(Icon::Check), img(Icon::Cross)]));
    }

    fn image(cover: &[u8]) -> Image {
        let data: Vec<u8> = cover.iter().flat_map(|&a| [255, 255, 255, a]).collect();
        Image::new(Extent3d { width: N, height: N, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD)
    }

    /// The five-pointed star (unit square, y down), scaled about the centre.
    fn star(scale: f32) -> [Vec2; 10] {
        std::array::from_fn(|k| {
            let r = if k % 2 == 0 { 0.48 } else { 0.2 } * scale;
            let a = std::f32::consts::PI * (k as f32 / 5.0) - std::f32::consts::FRAC_PI_2;
            Vec2::new(0.5, 0.53) + Vec2::new(a.cos(), a.sin()) * r
        })
    }

    fn inside(poly: &[Vec2], p: Vec2) -> bool {
        let mut c = false;
        let mut j = poly.len() - 1;
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[j]);
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                c = !c;
            }
            j = i;
        }
        c
    }

    fn seg(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0);
        p.distance(a + ab * t)
    }

    /// Coverage (0..255) per pixel.
    pub fn raster(i: Icon) -> Vec<u8> {
        let (outer, inner) = (star(1.0), star(0.62));
        let hit = |p: Vec2| match i {
            Icon::Star => inside(&outer, p),
            Icon::StarHollow => inside(&outer, p) && !inside(&inner, p),
            Icon::Check => seg(p, Vec2::new(0.16, 0.52), Vec2::new(0.4, 0.76)).min(seg(p, Vec2::new(0.4, 0.76), Vec2::new(0.86, 0.24))) < 0.085,
            Icon::Cross => seg(p, Vec2::new(0.2, 0.2), Vec2::new(0.8, 0.8)).min(seg(p, Vec2::new(0.8, 0.2), Vec2::new(0.2, 0.8))) < 0.085,
        };
        let mut out = vec![0u8; (N * N) as usize];
        for y in 0..N {
            for x in 0..N {
                let mut n = 0;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let p = Vec2::new((x * SS + sx) as f32 + 0.5, (y * SS + sy) as f32 + 0.5) / (N * SS) as f32;
                        n += hit(p) as u32;
                    }
                }
                out[(y * N + x) as usize] = (n * 255 / (SS * SS)) as u8;
            }
        }
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn icons_have_ink_and_a_hollow_star_has_a_hole() {
            for i in [Icon::Star, Icon::StarHollow, Icon::Check, Icon::Cross] {
                let r = raster(i);
                let ink = r.iter().filter(|&&a| a > 128).count();
                assert!(ink > 200 && ink < (N * N) as usize / 2, "{i:?}: {ink}");
            }
            let full = raster(Icon::Star);
            let hollow = raster(Icon::StarHollow);
            let centre = (N / 2 * N + N / 2) as usize;
            assert_eq!(full[centre], 255);
            assert_eq!(hollow[centre], 0);
        }
    }
}
