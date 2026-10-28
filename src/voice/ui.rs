//! Corner indicator (top left), shown while in a session: your mic level and whether you are
//! transmitting, and a "talking" light for your teammate.

use bevy::prelude::*;
use engine::flow::has_visuals;

use super::capture::MicStatus;
use super::net::VoiceRole;
use super::{TalkMode, VoiceSettings, VoiceState};

#[derive(Component)]
struct Panel;
#[derive(Component)]
struct TxDot;
#[derive(Component)]
struct Meter;
#[derive(Component)]
struct Hint;
#[derive(Component)]
struct RemoteDot;
#[derive(Component)]
struct RemoteLabel;

const OFF: Color = Color::srgba(1.0, 1.0, 1.0, 0.18);
const TX: Color = Color::srgb(0.95, 0.25, 0.2);
const TALK: Color = Color::srgb(0.3, 0.9, 0.45);
const METER_LOW: Color = Color::srgb(0.35, 0.8, 0.45);
const METER_HOT: Color = Color::srgb(0.95, 0.7, 0.2);

pub(super) fn build(app: &mut App) {
    app.add_systems(Update, (spawn_panel, update_panel).chain().after(super::VoiceSet).run_if(has_visuals));
}

fn dot(c: Color) -> impl Bundle {
    (Node { width: px(10), height: px(10), border_radius: BorderRadius::all(px(5)), ..default() }, BackgroundColor(c))
}

fn label(text: &str, size: f32) -> impl Bundle {
    (Text::new(text), TextFont { font_size: bevy::text::FontSize::Px(size), ..default() }, TextColor(Color::srgba(1.0, 1.0, 1.0, 0.85)))
}

fn spawn_panel(mut commands: Commands, q: Query<(), With<Panel>>) {
    if !q.is_empty() {
        return;
    }
    let row = || Node { flex_direction: FlexDirection::Row, align_items: AlignItems::Center, column_gap: px(8), ..default() };
    commands
        .spawn((
            Panel,
            Name::new("Voice indicator"),
            Node {
                position_type: PositionType::Absolute,
                // Top-left: top-right holds the driver's guide notice and the guide's panel.
                left: px(18),
                top: px(16),
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                padding: UiRect::axes(px(12), px(8)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
            Visibility::Hidden,
        ))
        .with_children(|p| {
            p.spawn(row()).with_children(|r| {
                r.spawn((TxDot, dot(OFF)));
                r.spawn(label("MIC", 14.0));
                r.spawn((Node { width: px(90), height: px(6), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12))))
                    .with_children(|m| {
                        m.spawn((Meter, Node { width: Val::Percent(0.0), height: Val::Percent(100.0), border_radius: BorderRadius::all(px(3)), ..default() }, BackgroundColor(METER_LOW)));
                    });
            });
            p.spawn((Hint, label("", 12.0)));
            p.spawn(row()).with_children(|r| {
                r.spawn((RemoteDot, dot(OFF)));
                r.spawn((RemoteLabel, label("", 14.0)));
            });
        });
}

/// −60..0 dBFS → 0..100 %.
fn meter_pct(db: f32) -> f32 {
    ((db + 60.0) / 60.0).clamp(0.0, 1.0) * 100.0
}

#[allow(clippy::type_complexity)]
fn update_panel(
    st: Res<VoiceState>,
    settings: Res<VoiceSettings>,
    mut panel: Query<&mut Visibility, With<Panel>>,
    mut colors: ParamSet<(Query<&mut BackgroundColor, With<TxDot>>, Query<&mut BackgroundColor, With<RemoteDot>>, Query<(&mut Node, &mut BackgroundColor), With<Meter>>)>,
    mut texts: ParamSet<(Query<&mut Text, With<Hint>>, Query<&mut Text, With<RemoteLabel>>)>,
) {
    let show = settings.enabled && st.role != VoiceRole::Offline;
    for mut v in &mut panel {
        v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    if !show {
        return;
    }
    for mut c in &mut colors.p0() {
        c.set_if_neq(BackgroundColor(if st.transmitting { TX } else { OFF }));
    }
    for mut c in &mut colors.p1() {
        c.set_if_neq(BackgroundColor(if st.remote_talking { TALK } else { OFF }));
    }
    for (mut n, mut c) in &mut colors.p2() {
        let w = Val::Percent(meter_pct(st.mic_level_db));
        if n.width != w {
            n.width = w;
        }
        c.set_if_neq(BackgroundColor(if st.mic_level_db > -12.0 { METER_HOT } else { METER_LOW }));
    }
    let hint = match st.mic_status {
        MicStatus::NoDevice => "No microphone found",
        MicStatus::Failed => "Microphone unavailable",
        MicStatus::Silent => "Mic silent: check permission",
        MicStatus::Starting => "Starting microphone…",
        _ if st.mode == TalkMode::PushToTalk => {
            if st.transmitting {
                "Talking"
            } else {
                "Hold V / LB to talk"
            }
        }
        _ => {
            if st.transmitting {
                "Open mic: talking"
            } else {
                "Open mic"
            }
        }
    };
    for mut t in &mut texts.p0() {
        if t.0 != hint {
            t.0 = hint.to_string();
        }
    }
    let remote = match (st.role, st.remote_talking) {
        (VoiceRole::Driver, true) => "GUIDE talking",
        (VoiceRole::Driver, false) => "GUIDE",
        (_, true) => "DRIVER talking",
        (_, false) => "DRIVER",
    };
    for mut t in &mut texts.p1() {
        if t.0 != remote {
            t.0 = remote.to_string();
        }
    }
}
