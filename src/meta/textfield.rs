//! A one-line text field for the UI kit (names, the driver's address): a focusable button
//! that starts editing on Enter / click, takes typed characters, Backspace deletes, Enter
//! commits, Esc cancels. While editing, menu navigation keys are swallowed.
//!
//! The value lives wherever the field's [`TextKey`] says ([`super::Profile`] fields); this
//! module only edits a copy and writes it back on commit.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use engine::ui::UiSet;
use engine::ui::prelude::*;
use engine::ui::widgets::{ActionButton, Focusable, Look, NavQueue, secondary_look};

use super::Profile;

/// Which profile string a field edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKey {
    Driver,
    Guide,
    Address,
}

impl TextKey {
    pub fn action(self) -> MenuAction {
        MenuAction::Custom(match self {
            TextKey::Driver => "edit:driver",
            TextKey::Guide => "edit:guide",
            TextKey::Address => "edit:address",
        })
    }
    pub fn from_action(id: &str) -> Option<TextKey> {
        Some(match id {
            "edit:driver" => TextKey::Driver,
            "edit:guide" => TextKey::Guide,
            "edit:address" => TextKey::Address,
            _ => return None,
        })
    }
    pub fn max_len(self) -> usize {
        match self {
            TextKey::Address => 64,
            _ => 24,
        }
    }
    pub fn get(self, p: &Profile) -> &str {
        match self {
            TextKey::Driver => &p.driver_name,
            TextKey::Guide => &p.guide_name,
            TextKey::Address => &p.join_address,
        }
    }
    pub fn set(self, p: &mut Profile, v: String) {
        match self {
            TextKey::Driver => p.driver_name = v,
            TextKey::Guide => p.guide_name = v,
            TextKey::Address => p.join_address = v,
        }
    }
    fn accepts(self, c: char) -> bool {
        match self {
            TextKey::Address => c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '-' | '[' | ']'),
            _ => !c.is_control() && c != '|',
        }
    }
}

/// The field being edited, with its working copy.
#[derive(Resource, Clone, Debug, Default)]
pub struct Editing(pub Option<(TextKey, String)>);

/// The value text inside a field.
#[derive(Component, Clone, Copy, Debug)]
pub struct TextValue(pub TextKey);

/// The field's box (border shows edit mode).
#[derive(Component, Clone, Copy, Debug)]
pub struct TextBox(pub TextKey);

pub struct TextFieldPlugin;

impl Plugin for TextFieldPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Editing>()
            .add_systems(Update, type_text.before(UiSet::Input).run_if(engine::ui::ui_visible))
            .add_systems(Update, paint_fields.after(UiSet::Paint).run_if(engine::ui::ui_visible));
    }
}

/// A labelled text field: label above, box with the value.
pub fn text_field(p: &mut ChildSpawnerCommands, t: &Theme, label_text: &str, key: TextKey, value: &str, order: u32, width: f32) {
    p.spawn((Node { flex_direction: FlexDirection::Column, row_gap: px(GAP_XS + 2.0), ..default() }, Pickable::IGNORE)).with_children(|c| {
        c.spawn(eyebrow(label_text, t.text_muted));
        c.spawn((
            Button,
            ActionButton(key.action().into()),
            Focusable { order },
            TextBox(key),
            secondary_look(t),
            Node {
                width: px(width),
                height: px(56),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(18)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(Color::NONE),
        ))
        .with_children(|b| {
            b.spawn((TextValue(key), text(value.to_string(), body_bold(24.0), t.text)));
        });
    });
}

fn type_text(mut keys: MessageReader<KeyboardInput>, mut editing: ResMut<Editing>, mut nav: ResMut<NavQueue>, mut profile: ResMut<Profile>) {
    let Some((key, buf)) = editing.0.as_mut() else {
        keys.clear();
        return;
    };
    let key = *key;
    // Typing must not move the focus or close the screen.
    nav.0.clear();
    let mut done: Option<bool> = None;
    for ev in keys.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Enter => done = Some(true),
            Key::Escape => done = Some(false),
            Key::Backspace => {
                buf.pop();
            }
            Key::Space if buf.chars().count() < key.max_len() && key != TextKey::Address => buf.push(' '),
            Key::Character(s) => {
                for c in s.chars() {
                    if key.accepts(c) && buf.chars().count() < key.max_len() {
                        buf.push(c);
                    }
                }
            }
            _ => {}
        }
    }
    match done {
        Some(true) => {
            let v = if key == TextKey::Address { buf.trim().to_string() } else { super::board::clean_name(buf) };
            if !v.is_empty() {
                key.set(&mut profile, v);
            }
            editing.0 = None;
        }
        Some(false) => editing.0 = None,
        None => {}
    }
}

/// Starts editing `key` (from its button).
pub fn begin(world: &mut World, key: TextKey) {
    let v = key.get(world.resource::<Profile>()).to_string();
    world.resource_mut::<Editing>().0 = Some((key, v));
}

fn paint_fields(
    editing: Res<Editing>,
    profile: Res<Profile>,
    theme: Res<Theme>,
    time: Res<Time<Real>>,
    mut values: Query<(&TextValue, &mut Text)>,
    mut boxes: Query<(&TextBox, &mut BorderColor, &mut BackgroundColor), With<Look>>,
) {
    let caret = ((time.elapsed_secs() * 2.0) as u32).is_multiple_of(2);
    for (v, mut text) in &mut values {
        let want = match &editing.0 {
            Some((k, buf)) if *k == v.0 => format!("{buf}{}", if caret { "|" } else { " " }),
            _ => v.0.get(&profile).to_string(),
        };
        if text.0 != want {
            text.0 = want;
        }
    }
    // While editing, the box shows the accent outline whatever the hover state says.
    for (b, mut border, mut bg) in &mut boxes {
        if editing.0.as_ref().is_some_and(|(k, _)| *k == b.0) {
            *border = BorderColor::all(theme.accent);
            *bg = BackgroundColor(theme.accent.with_alpha(0.08));
        }
    }
}
