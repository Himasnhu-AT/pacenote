//! The driver's only sign of the guide: a small label top-right ("GUIDE CONNECTED 18 MS",
//! "WAITING FOR GUIDE  PORT 7777", "GUIDE DISCONNECTED"), brighter for a few seconds after it
//! changes. The driver never sees the map or the corner list.

use std::fmt::Write;

use bevy::prelude::*;

use super::is_driver;
use crate::net_session::Session;

/// Seconds a change stays highlighted.
const FRESH_S: f64 = 4.0;

#[derive(Component)]
struct GuideNotice;

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, update.run_if(is_driver));
}

fn update(mut commands: Commands, time: Res<Time<Real>>, session: Res<Session>, mut q: Query<(&mut Text, &mut TextColor), With<GuideNotice>>, mut s: Local<String>) {
    let Ok((mut text, mut color)) = q.single_mut() else {
        commands.spawn((
            Name::new("guide notice"),
            GuideNotice,
            Text::new(""),
            engine::ui::theme::label(16.0),
            TextColor(Color::WHITE),
            Node { position_type: PositionType::Absolute, right: Val::Px(24.0), top: Val::Px(20.0), ..default() },
        ));
        return;
    };
    let fresh = time.elapsed_secs_f64() - session.changed_at < FRESH_S && session.changed_at > 0.0;
    s.clear();
    let c = if session.guide.is_some() {
        let _ = write!(s, "GUIDE CONNECTED  {:.0} MS", session.rtt_ms);
        Color::srgb(0.4, 0.85, 0.5)
    } else if session.last_drop.is_some() {
        s.push_str("GUIDE DISCONNECTED");
        Color::srgb(0.95, 0.35, 0.35)
    } else {
        s.push_str("WAITING FOR GUIDE");
        if let Some(a) = session.addr {
            let _ = write!(s, "  PORT {}", a.port());
        }
        Color::srgb(0.75, 0.78, 0.82)
    };
    let c = c.with_alpha(if fresh { 1.0 } else { 0.6 });
    if text.0 != *s {
        text.0.clone_from(&s);
    }
    if color.0 != c {
        color.0 = c;
    }
}
