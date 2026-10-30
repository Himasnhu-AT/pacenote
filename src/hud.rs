//! Driver HUD: speed, gear, rpm bar, stage clock, damage, countdown, result.

use bevy::prelude::*;

use crate::drive::{DrivenCar, StageRun};
use crate::world::StageWorld;

#[derive(Component)]
struct HudText;

#[derive(Component)]
struct HudCenter;

/// Damage readout (top right), shown once the car has any.
#[derive(Component)]
struct HudDamage;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_hud, update_hud).chain().run_if(resource_exists::<StageWorld>));
    }
}

fn spawn_hud(mut commands: Commands, q: Query<(), With<HudText>>) {
    let font = |px: f32| TextFont { font_size: bevy::text::FontSize::Px(px), ..default() };
    if !q.is_empty() {
        return;
    }
    commands.spawn((
        HudText,
        DespawnOnExit(engine::flow::AppState::InGame),
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(26.0), ..default() },
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, left: Val::Px(24.0), bottom: Val::Px(20.0), ..default() },
    ));
    commands.spawn((
        HudCenter,
        DespawnOnExit(engine::flow::AppState::InGame),
        Text::new(""),
        TextFont { font_size: bevy::text::FontSize::Px(64.0), ..default() },
        TextColor(Color::srgb(1.0, 0.85, 0.3)),
        Node { position_type: PositionType::Absolute, top: Val::Percent(30.0), width: Val::Percent(100.0), justify_content: JustifyContent::Center, ..default() },
        TextLayout::justify(Justify::Center),
    ));
    commands.spawn((
        HudDamage,
        DespawnOnExit(engine::flow::AppState::InGame),
        Text::new(""),
        font(20.0),
        TextColor(Color::WHITE),
        Node { position_type: PositionType::Absolute, right: Val::Px(24.0), top: Val::Px(20.0), ..default() },
        TextLayout::justify(Justify::Right),
    ));
}

/// `ENGINE  ######....  62%`-style bar for a part.
fn part(out: &mut String, name: &str, d: f32) {
    use std::fmt::Write;
    let n = (d * 10.0).round() as usize;
    let _ = writeln!(out, "{name:<10} {}{} {:>3.0}%", "#".repeat(n.min(10)), ".".repeat(10 - n.min(10)), d * 100.0);
}

fn clock(t: f32) -> String {
    let m = (t / 60.0).floor();
    format!("{}:{:06.3}", m as u32, t - m * 60.0)
}

type Only<A, B, C> = (With<A>, Without<B>, Without<C>);

fn update_hud(
    run: Res<StageRun>,
    stage: Res<StageWorld>,
    cars: Query<&DrivenCar>,
    mut text: Query<&mut Text, Only<HudText, HudCenter, HudDamage>>,
    mut center: Query<(&mut Text, &mut TextColor), Only<HudCenter, HudText, HudDamage>>,
    mut damage: Query<(&mut Text, &mut TextColor), Only<HudDamage, HudText, HudCenter>>,
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
    let d = car.damage;
    if let Ok((mut t, mut col)) = damage.single_mut() {
        let mut s = String::new();
        if d.worst() > 0.02 {
            s.push_str("DAMAGE\n");
            part(&mut s, "ENGINE", d.engine);
            part(&mut s, "STEERING", d.steering);
            part(&mut s, "SUSPENSION", d.suspension);
        }
        if t.0 != s {
            t.0 = s;
        }
        // White → amber → red with the worst part.
        let w = d.worst();
        col.0 = if w < 0.35 { Color::WHITE } else if w < 0.7 { Color::srgb(1.0, 0.75, 0.25) } else { Color::srgb(1.0, 0.3, 0.25) };
    }
    if let Ok((mut t, mut col)) = center.single_mut() {
        col.0 = if run.retired { Color::srgb(1.0, 0.3, 0.25) } else { Color::srgb(1.0, 0.85, 0.3) };
        t.0 = if run.retired {
            "RETIRED\nThe car is too damaged to continue\nEsc: restart or leave".into()
        } else if run.countdown > 0.0 {
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
