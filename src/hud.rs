//! Driver HUD: speed, gear, rpm bar, stage clock, countdown, result.

use bevy::prelude::*;

use crate::drive::{DrivenCar, StageRun};
use crate::world::StageWorld;

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct HudCenter;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_hud, update_hud).chain().run_if(resource_exists::<StageWorld>));
    }
}

fn spawn_hud(mut commands: Commands, q: Query<(), With<HudText>>, mut done: Local<bool>) {
    if *done || !q.is_empty() {
        return;
    }
    *done = true;
    commands.spawn((
        HudText,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(26.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, left: Val::Px(24.0), bottom: Val::Px(20.0), ..default() },
    ));
    commands.spawn((
        HudCenter,
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(64.0), ..default() },
        TextColor(Color::srgb(1.0, 0.85, 0.3)),
        Node { position_type: PositionType::Absolute, top: Val::Percent(30.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
}

fn clock(t: f32) -> String {
    let m = (t / 60.0).floor();
    format!("{}:{:06.3}", m as u32, t - m * 60.0)
}

fn update_hud(
    run: Res<StageRun>,
    stage: Res<StageWorld>,
    cars: Query<&DrivenCar>,
    mut text: Query<&mut Text, (With<HudText>, Without<HudCenter>)>,
    mut center: Query<&mut Text, (With<HudCenter>, Without<HudText>)>,
) {
    let Ok(c) = cars.single() else { return };
    let car = &c.car;
    let kmh = (car.speed() * 3.6).abs();
    let gear = if car.gear < 0 { "R".into() } else { car.gear.to_string() };
    let rpm_bar = {
        let n = ((car.rpm / 7600.0) * 20.0).round() as usize;
        format!("{}{}", "|".repeat(n.min(20)), ".".repeat(20 - n.min(20)))
    };
    let total = run.time + run.penalty;
    let pen = if run.penalty > 0.0 { format!("  (+{:.0} s)", run.penalty) } else { String::new() };
    let km = run.progress / 1000.0;
    if let Ok(mut t) = text.single_mut() {
        t.0 = format!(
            "{kmh:>3.0} km/h   gear {gear}\n{rpm_bar} {:>4.0} rpm\n{}   {}{pen}   {:.2} / {:.2} km",
            car.rpm,
            stage.def.name,
            clock(total),
            km,
            stage.road.length() / 1000.0
        );
    }
    if let Ok(mut t) = center.single_mut() {
        t.0 = if run.countdown > 0.0 {
            format!("{}", run.countdown.ceil() as u32)
        } else if let Some(f) = run.finished {
            format!("FINISH  {}", clock(f))
        } else if run.time < 1.0 {
            "GO".into()
        } else {
            String::new()
        };
    }
}
