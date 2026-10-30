//! The always-on-top widget (A11): a small borderless window that shows one
//! controller's live state above other windows, for streaming or checking
//! a pad while playing.
//!
//! It is a second egui viewport: undecorated, transparent, always on top,
//! out of the taskbar, optionally click-through. Dragging moves it while it
//! takes clicks. Platform limits (`apps/desktop/README.md`): Wayland lets
//! no regular window stay on top by itself, and exclusive full-screen games
//! cover every window.

use anygamepad::model::{Axis, Button, GamepadState, PowerState};
use anygamepad::provider::DeviceId;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};

use crate::devices::{DeviceView, Snapshot};

/// How the widget draws a controller.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WidgetSkin {
    /// A controller outline with every control.
    #[default]
    Pad,
    /// Sticks, triggers and the names of the buttons pressed.
    Compact,
}

/// Lit controls.
const ON: Color32 = Color32::from_rgb(0x00, 0xd0, 0xeb);
/// Controls at rest.
const OFF: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xb5);
/// Text.
const TEXT: Color32 = Color32::from_rgb(0xec, 0xec, 0xf4);

/// What the widget needs from the settings.
pub struct WidgetOptions {
    /// Draw style.
    pub skin: WidgetSkin,
    /// Background opacity, 0-100.
    pub opacity: u8,
    /// Clicks go to the window below.
    pub click_through: bool,
}

/// The widget's state between frames.
#[derive(Default)]
pub struct Widget {
    /// The controller shown; the first one when unset or gone.
    pub device: Option<DeviceId>,
}

impl Widget {
    /// Shows the widget window for this frame. Returns `false` when the
    /// person closed it (Alt+F4 or the window manager).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        options: &WidgetOptions,
        snapshot: &Snapshot,
    ) -> bool {
        let device = snapshot
            .devices
            .iter()
            .find(|d| Some(d.id) == self.device)
            .or_else(|| snapshot.devices.first());
        let builder = egui::ViewportBuilder::default()
            .with_title("AnyGamepad widget")
            .with_app_id("anygamepad-widget")
            .with_inner_size([300.0, 180.0])
            .with_min_inner_size([160.0, 100.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(true)
            .with_mouse_passthrough(options.click_through);
        let mut open = true;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("anygamepad-widget"),
            builder,
            |ctx, _class| {
                let alpha = (f32::from(options.opacity.min(100)) / 100.0 * 255.0).round() as u8;
                let frame = egui::Frame::NONE
                    .fill(Color32::from_rgba_unmultiplied(0x14, 0x14, 0x1c, alpha))
                    .corner_radius(12)
                    .inner_margin(8);
                egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
                    let area = ui.max_rect();
                    let response = ui.interact(area, ui.id().with("drag"), Sense::click_and_drag());
                    if response.drag_started() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                    match device {
                        Some(device) => match options.skin {
                            WidgetSkin::Pad => pad(ui, area, device),
                            WidgetSkin::Compact => compact(ui, area, device),
                        },
                        None => {
                            ui.painter().text(
                                area.center(),
                                egui::Align2::CENTER_CENTER,
                                "No controller",
                                egui::FontId::proportional(14.0),
                                TEXT,
                            );
                        }
                    }
                });
                if ctx.input(|i| i.viewport().close_requested()) {
                    open = false;
                }
            },
        );
        open
    }
}

/// The name and battery line at the top.
fn title(ui: &egui::Ui, area: Rect, device: &DeviceView) -> f32 {
    let name = device
        .info
        .identity
        .product_name
        .as_deref()
        .unwrap_or("Controller");
    let power = device.state.power;
    let battery = match (power.state, power.percent) {
        (PowerState::Unknown, _) => String::new(),
        (state, Some(percent)) => {
            let plug = if state == PowerState::Charging {
                " +"
            } else {
                ""
            };
            format!("  {percent}%{plug}")
        }
        (_, None) => String::new(),
    };
    ui.painter().text(
        area.left_top(),
        egui::Align2::LEFT_TOP,
        format!("{name}{battery}"),
        egui::FontId::proportional(12.0),
        TEXT,
    );
    16.0
}

/// A control's colour.
fn lit(pressed: bool) -> Color32 {
    if pressed { ON } else { OFF }
}

/// A stick: ring, position dot, filled when clicked.
fn stick(painter: &egui::Painter, center: Pos2, radius: f32, x: f32, y: f32, clicked: bool) {
    painter.circle_stroke(center, radius, Stroke::new(1.5_f32, OFF));
    let dot = pos2(center.x + x * radius * 0.7, center.y - y * radius * 0.7);
    let size = radius * 0.35;
    if clicked {
        painter.circle_filled(dot, size, ON);
    } else {
        painter.circle_stroke(dot, size, Stroke::new(2.0_f32, ON));
    }
}

/// A trigger: a bar filling upwards.
fn trigger(painter: &egui::Painter, rect: Rect, value: f32) {
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0_f32, OFF),
        egui::StrokeKind::Inside,
    );
    let filled = Rect::from_min_max(
        pos2(
            rect.left(),
            rect.bottom() - rect.height() * value.clamp(0.0, 1.0),
        ),
        rect.max,
    );
    painter.rect_filled(filled, 3.0, ON);
}

/// The full skin: sticks, d-pad, face buttons, shoulders, triggers, menus.
fn pad(ui: &egui::Ui, area: Rect, device: &DeviceView) {
    let top = title(ui, area, device);
    let area = Rect::from_min_max(pos2(area.left(), area.top() + top), area.max);
    let painter = ui.painter_at(area);
    let s: &GamepadState = &device.state;
    let at = |x: f32, y: f32| {
        pos2(
            area.left() + x * area.width(),
            area.top() + y * area.height(),
        )
    };
    let unit = area.width().min(area.height() * 1.6) / 16.0;

    // Triggers and shoulders along the top edge.
    let bar = vec2(unit * 1.2, unit * 2.4);
    trigger(
        &painter,
        Rect::from_center_size(at(0.08, 0.22), bar),
        s.axis(Axis::LeftTrigger),
    );
    trigger(
        &painter,
        Rect::from_center_size(at(0.92, 0.22), bar),
        s.axis(Axis::RightTrigger),
    );
    let shoulder = vec2(unit * 3.0, unit * 0.8);
    for (x, button) in [(0.2, Button::LeftShoulder), (0.8, Button::RightShoulder)] {
        painter.rect_filled(
            Rect::from_center_size(at(x, 0.08), shoulder),
            3.0,
            lit(s.pressed(button)),
        );
    }

    // Sticks.
    let left = s.left_stick();
    let right = s.right_stick();
    stick(
        &painter,
        at(0.24, 0.42),
        unit * 1.8,
        left.x,
        left.y,
        s.pressed(Button::LeftStick),
    );
    stick(
        &painter,
        at(0.64, 0.75),
        unit * 1.8,
        right.x,
        right.y,
        s.pressed(Button::RightStick),
    );

    // D-pad.
    let dpad = at(0.36, 0.75);
    let arm = unit * 0.9;
    for (offset, button) in [
        (vec2(0.0, -1.0), Button::DPadUp),
        (vec2(0.0, 1.0), Button::DPadDown),
        (vec2(-1.0, 0.0), Button::DPadLeft),
        (vec2(1.0, 0.0), Button::DPadRight),
    ] {
        let center = dpad + offset * arm;
        painter.rect_filled(
            Rect::from_center_size(center, Vec2::splat(arm * 0.95)),
            2.0,
            lit(s.pressed(button)),
        );
    }

    // Face buttons.
    let face = at(0.76, 0.42);
    for (offset, button) in [
        (vec2(0.0, 1.0), Button::South),
        (vec2(1.0, 0.0), Button::East),
        (vec2(-1.0, 0.0), Button::West),
        (vec2(0.0, -1.0), Button::North),
    ] {
        painter.circle_filled(
            face + offset * unit * 1.1,
            unit * 0.55,
            lit(s.pressed(button)),
        );
    }

    // Menus and home.
    for (x, button) in [(0.42, Button::Select), (0.58, Button::Start)] {
        painter.rect_filled(
            Rect::from_center_size(at(x, 0.36), vec2(unit * 1.0, unit * 0.5)),
            2.0,
            lit(s.pressed(button)),
        );
    }
    painter.circle_filled(at(0.5, 0.52), unit * 0.5, lit(s.pressed(Button::Home)));
}

/// The compact skin: two sticks, two triggers, the buttons pressed.
fn compact(ui: &egui::Ui, area: Rect, device: &DeviceView) {
    let top = title(ui, area, device);
    let area = Rect::from_min_max(pos2(area.left(), area.top() + top), area.max);
    let painter = ui.painter_at(area);
    let s = &device.state;
    let radius = (area.height() * 0.3).min(area.width() * 0.12);
    let y = area.top() + radius + 4.0;
    let left = s.left_stick();
    let right = s.right_stick();
    stick(
        &painter,
        pos2(area.left() + radius + 2.0, y),
        radius,
        left.x,
        left.y,
        s.pressed(Button::LeftStick),
    );
    stick(
        &painter,
        pos2(area.left() + 3.0 * radius + 10.0, y),
        radius,
        right.x,
        right.y,
        s.pressed(Button::RightStick),
    );
    let bar = vec2(radius * 0.4, radius * 2.0);
    let x = area.left() + 4.0 * radius + 24.0;
    trigger(
        &painter,
        Rect::from_center_size(pos2(x, y), bar),
        s.axis(Axis::LeftTrigger),
    );
    trigger(
        &painter,
        Rect::from_center_size(pos2(x + bar.x * 2.0, y), bar),
        s.axis(Axis::RightTrigger),
    );
    let pressed: Vec<&str> = s.buttons.iter().map(|b| b.as_str()).collect();
    painter.text(
        pos2(area.left(), area.top() + 2.0 * radius + 12.0),
        egui::Align2::LEFT_TOP,
        if pressed.is_empty() {
            "·".to_owned()
        } else {
            pressed.join(" ")
        },
        egui::FontId::monospace(11.0),
        ON,
    );
}
