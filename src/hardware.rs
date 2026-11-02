//! Fit the game to the machine: a first-launch graphics quality from the hardware.
//!
//! A base MacBook Air (M1/M2, 8 GB, ~2.5k-pixel Retina panel) at High in borderless fullscreen
//! ran out of memory and GPU and froze the whole Mac. The engine's governor only steps down
//! after frames are already missed and never touches what the scenery costs, so the starting
//! point has to fit.

use bevy::prelude::*;
use engine::settings::{CoreSettings, GraphicsQuality, SettingsStore};

pub struct HardwarePlugin;

impl Plugin for HardwarePlugin {
    fn build(&self, app: &mut App) {
        quality_on_first_run(app);
    }
}

/// What we know about the machine.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Machine {
    pub ram_gb: Option<f32>,
    /// CPU/SoC name, e.g. "Apple M2" or "Apple M3 Pro".
    pub chip: Option<String>,
}

impl Machine {
    pub fn detect() -> Machine {
        Machine { ram_gb: engine::render_budget::governor::ram_mb().map(|mb| mb / 1024.0), chip: chip_name() }
    }

    /// A base Apple Silicon chip (M1, M2 …, not Pro / Max / Ultra): 7–10 GPU cores.
    fn base_apple_chip(&self) -> bool {
        self.chip.as_deref().is_some_and(|c| c.starts_with("Apple M") && !["Pro", "Max", "Ultra"].iter().any(|t| c.contains(t)))
    }

    /// The quality a fresh install starts at.
    pub fn quality(&self) -> GraphicsQuality {
        match self.ram_gb {
            Some(gb) if gb <= 8.5 => GraphicsQuality::Low,
            _ if self.base_apple_chip() => GraphicsQuality::Medium,
            Some(gb) if gb <= 16.5 => GraphicsQuality::Medium,
            _ => GraphicsQuality::High,
        }
    }
}

fn chip_name() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "machdep.cpu.brand_string"]).output().ok()?;
        let s = String::from_utf8(out.stdout).ok()?.trim().to_string();
        (!s.is_empty()).then_some(s)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Only when the settings file has never stored a quality, so the player's choice sticks.
fn quality_on_first_run(app: &mut App) {
    let Some(path) = app.world().get_resource::<SettingsStore>().map(|s| s.path.clone()) else { return };
    if crate::core_setting_saved(std::fs::read_to_string(path).ok().as_deref(), "quality") {
        return;
    }
    let machine = Machine::detect();
    let q = machine.quality();
    info!("first run: graphics quality {q:?} for {machine:?}");
    if let Some(mut core) = app.world_mut().get_resource_mut::<CoreSettings>() {
        core.quality = q;
    }
    if let Some(mut store) = app.world_mut().get_resource_mut::<SettingsStore>() {
        store.mark_dirty();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(gb: f32, chip: &str) -> Machine {
        Machine { ram_gb: Some(gb), chip: Some(chip.into()) }
    }

    #[test]
    fn quality_fits_the_machine() {
        assert_eq!(m(8.0, "Apple M1").quality(), GraphicsQuality::Low);
        assert_eq!(m(8.0, "Apple M3 Pro").quality(), GraphicsQuality::Low, "8 GB is the limit whatever the chip");
        assert_eq!(m(16.0, "Apple M2").quality(), GraphicsQuality::Medium);
        assert_eq!(m(24.0, "Apple M4").quality(), GraphicsQuality::Medium, "base chip GPU");
        assert_eq!(m(16.0, "Apple M1 Pro").quality(), GraphicsQuality::Medium);
        assert_eq!(m(24.0, "Apple M5 Pro").quality(), GraphicsQuality::High);
        assert_eq!(m(32.0, "AMD Ryzen 7 5800X").quality(), GraphicsQuality::High);
        assert_eq!(Machine::default().quality(), GraphicsQuality::High, "unknown machine: the engine default");
    }
}
