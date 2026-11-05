//! The confirm modal: "Quit to desktop?", "Buy Gravel engine?", "Sell the Wren?".
//!
//! **Owner: stream E (UI).** Design: `docs/festival/UX_PASS.md` §1 (Quit) and §4 (confirm
//! purchase).
//!
//! It is a screen on the stack ("confirm"), so Esc / Ⓑ cancels and focus returns to the item
//! that opened it. Its builder first builds the screen underneath, strips that screen's
//! buttons (so focus and clicks only reach the dialog), then lays a 60 % scrim and the dialog
//! over it. The confirming button is focused first (it is a deliberate second press); when
//! it's disabled (too few Notes) the dialog says why and Cancel takes focus.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use engine::ui::prelude::*;
use engine::ui::widgets::{ActionButton, Focusable, Look, secondary_look};

use crate::career::CareerRequest;

/// Screen id.
pub const SCREEN: &str = "confirm";

/// What the confirming button does.
#[derive(Clone, Debug, PartialEq)]
pub enum OnYes {
    Quit,
    /// Sent to the career (F applies it, saves, and toasts a refusal).
    Request(CareerRequest),
}

/// The open dialog.
#[derive(Resource, Clone, Debug)]
pub struct Confirm {
    pub kicker: String,
    pub title: String,
    /// Lines under the title ("7,000 N · balance after 5,000 N").
    pub lines: Vec<String>,
    pub yes: String,
    pub no: String,
    pub on_yes: OnYes,
    /// Why the confirming button is off ("Need 3,200 N more"); `None` = it's on.
    pub refused: Option<String>,
    /// Focus starts on the cancelling button (Quit: "Stay"). Purchases start on Buy.
    pub safe_default: bool,
}

/// Opens the dialog over the current screen.
pub fn ask(world: &mut World, c: Confirm) {
    world.insert_resource(c);
    world.write_message(ScreenRequest::Push(SCREEN));
}

/// The confirming button was pressed: close the dialog and do it.
pub fn yes(world: &mut World) {
    let Some(c) = world.remove_resource::<Confirm>() else { return };
    if c.refused.is_some() {
        world.write_message(crate::audio::ui::UiSound::Error);
        return;
    }
    world.write_message(ScreenRequest::Pop);
    match c.on_yes {
        OnYes::Quit => {
            world.write_message(engine::settings::SaveSettings);
            world.write_message(AppExit::Success);
        }
        OnYes::Request(r) => {
            info!("career request: {r:?}");
            world.write_message(r);
        }
    }
}

/// Left / Right move between the dialog's two buttons (as Up / Down do).
pub fn sideways(screens: Option<Res<Screens>>, mut q: ResMut<engine::ui::widgets::NavQueue>) {
    use engine::ui::widgets::NavInput;
    if screens.and_then(|s| s.top()) != Some(SCREEN) {
        return;
    }
    for i in q.0.iter_mut() {
        *i = match *i {
            NavInput::Left => NavInput::Up,
            NavInput::Right => NavInput::Down,
            x => x,
        };
    }
}

/// Built under the dialog: no intro, no live buttons.
#[derive(Resource)]
pub struct BuildingUnder;

pub fn build(world: &mut World, root: Entity) {
    let Some(c) = world.get_resource::<Confirm>().cloned() else {
        world.write_message(ScreenRequest::Pop);
        return;
    };
    // The screen underneath, as a still.
    let stack = world.get_resource::<Screens>().map(|s| s.stack.clone()).unwrap_or_default();
    if let Some(under) = stack.len().checked_sub(2).map(|i| stack[i])
        && let Some(builder) = world.resource::<engine::ui::screens::ScreenRegistry>().get(under)
    {
        world.insert_resource(BuildingUnder);
        builder(world, root);
        world.flush();
        world.remove_resource::<BuildingUnder>();
        let mut kids = Vec::new();
        let mut q = world.query::<&Children>();
        let mut todo = vec![root];
        while let Some(e) = todo.pop() {
            if let Ok(c) = q.get(world, e) {
                todo.extend(c.iter());
                kids.extend(c.iter());
            }
        }
        for e in kids {
            if let Ok(mut em) = world.get_entity_mut(e) {
                em.remove::<(Focusable, ActionButton, Button, Look)>();
            }
        }
    }
    let t = world.resource::<Theme>().clone();
    let glyphs = world.get_resource::<super::hub::Glyphs>().cloned().unwrap_or_default();
    let hints = engine::ui::device_hints(world, &[("A / D", "Choose"), ("Enter", "Confirm"), ("Esc", "Cancel")], &[("D-pad", "Choose"), ("A", "Confirm"), ("B", "Cancel")]);
    world.commands().entity(root).with_children(|p| {
        // The scrim takes the pointer, so nothing underneath lights up.
        p.spawn((fullscreen(), BackgroundColor(t.bg.with_alpha(0.6)), GlobalZIndex(30)));
        p.spawn((GlobalZIndex(31), Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..fullscreen() }, Pickable::IGNORE)).with_children(|m| {
            m.spawn((
                Node { width: px(640), flex_direction: FlexDirection::Column, row_gap: px(GAP_S + 2.0), padding: UiRect::all(px(32)), border: UiRect::top(px(3)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                BackgroundColor(t.raised),
                BorderColor::all(t.accent),
                Pickable::IGNORE,
            ))
            .with_children(|d| {
                d.spawn(eyebrow(&c.kicker, t.accent));
                d.spawn(title(&c.title, 46.0, t.text));
                for l in &c.lines {
                    d.spawn((text(l.clone(), body(SIZE_BODY), t.text_muted), Node { max_width: px(576), ..default() }));
                }
                if let Some(r) = &c.refused {
                    // A lock glyph and the reason: never colour alone.
                    d.spawn((Node { column_gap: px(GAP_S), align_items: AlignItems::Center, ..default() }, Pickable::IGNORE)).with_children(|row| {
                        super::hub::glyph(row, &glyphs, super::hub::Glyph::Lock, 20.0, t.bad);
                        row.spawn(text(r.clone(), body_bold(SIZE_BODY), t.bad));
                    });
                }
                d.spawn(spacer(GAP_S));
                d.spawn((Node { column_gap: px(GAP_M), ..default() }, Pickable::IGNORE)).with_children(|b| {
                    // Focus starts on the lowest order.
                    let (yes_order, no_order) = if c.safe_default || c.refused.is_some() { (2, 1) } else { (1, 2) };
                    if c.refused.is_none() {
                        action_button(b, &t, &c.yes, MenuAction::Custom("confirm_yes"), Some(yes_order), true, 300.0);
                    } else {
                        // Shown, but off: no action, not focusable.
                        b.spawn((
                            Node { width: px(300), height: px(64), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
                            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                            BorderColor::all(t.line),
                            Pickable::IGNORE,
                        ))
                        .with_children(|x| {
                            x.spawn(text(c.yes.to_uppercase(), label(28.0), t.text_dim));
                        });
                    }
                    cancel_button(b, &t, &c.no, no_order);
                });
                // Key hints inside the dialog (the screen underneath keeps its own footer).
                d.spawn((Node { column_gap: px(GAP_L), margin: UiRect::top(px(GAP_XS)), ..default() }, Pickable::IGNORE)).with_children(|h| {
                    for (k, a) in &hints {
                        key_hint(h, &t, k, a);
                    }
                });
            });
        });
    });
}

fn cancel_button(p: &mut ChildSpawnerCommands, t: &Theme, label_text: &str, order: u32) {
    p.spawn((
        Button,
        Hovered::default(),
        ActionButton(UiAction::Back),
        Focusable { order },
        secondary_look(t),
        Node { width: px(220), height: px(64), justify_content: JustifyContent::Center, align_items: AlignItems::Center, border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(RADIUS)), ..default() },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|b| {
        b.spawn(text(label_text.to_uppercase(), label(26.0), t.text));
    });
}
