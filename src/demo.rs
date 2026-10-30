//! The free demo (`--features demo`): three stages (one each of gravel, tarmac and snow), the
//! rest shown locked with a wishlist link; "Wishlist on Steam" in the main menu and after a
//! run. Without the feature everything here is a no-op and every stage is open.

use bevy::prelude::*;

/// This is the demo build.
pub const ACTIVE: bool = cfg!(feature = "demo");

/// Stages in the demo: Pinewater (gravel, pine forest), Thornback (tarmac, scrub), Frostline
/// (snow, snow forest).
pub const STAGES: [u32; 3] = [1, 6, 9];

/// The full game's Steam store page (set at build time with `PACENOTE_STEAM_URL`).
pub fn store_url() -> &'static str {
    option_env!("PACENOTE_STEAM_URL").unwrap_or("https://store.steampowered.com/search/?term=Pacenote")
}

/// Can this stage be driven in this build?
pub fn allowed(id: u32) -> bool {
    !ACTIVE || STAGES.contains(&id)
}

/// The nearest stage this build allows (the demo's first stage for a locked one).
pub fn clamp_stage(id: u32) -> u32 {
    if allowed(id) { id } else { STAGES[0] }
}

/// The stage after `id` that this build allows (wrapping round).
pub fn next_stage(id: u32, count: u32) -> u32 {
    (1..=count).map(|k| (id + k - 1) % count + 1).find(|&n| allowed(n)).unwrap_or(id)
}

/// Opens the store page in the browser (Steam's overlay would need the Steamworks SDK).
pub fn open_store(world: &mut World, from: &'static str) {
    crate::telemetry::event(world, "demo.wishlist", &[("where", crate::telemetry::Prop::Str(from))]);
    let url = store_url();
    let r = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "windows") {
        // `start` is a cmd built-in; the empty string is the window title.
        crate::codriver::speech::no_window(std::process::Command::new("cmd").args(["/C", "start", "", url])).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(url).spawn()
    };
    if let Err(e) = r {
        warn!("couldn't open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_stages_exist_and_clamp() {
        let ids: Vec<u32> = crate::stage::stages().iter().map(|d| d.id).collect();
        assert!(STAGES.iter().all(|s| ids.contains(s)));
        for id in 1..=20 {
            assert!(allowed(clamp_stage(id)));
        }
    }
}
