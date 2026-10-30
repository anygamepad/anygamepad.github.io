//! The Devices page (A2): the list of controllers, and the selected one's
//! identity, canonical state and raw values side by side.

use anygamepad::model::raw::{Hat, RawSource};
use anygamepad::model::{Axis, Button, GamepadState, OutputCommand, PowerState, Rumble, Stick};
use anygamepad::provider::DeviceId;
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, Vec2};

use crate::devices::{Command, DeviceView, Devices, MappingSource, Snapshot, can_rumble, summary};

/// The page's state between frames.
#[derive(Default)]
pub struct DevicesPage {
    selected: Option<DeviceId>,
}

impl DevicesPage {
    /// Draws the page.
    pub fn show(&mut self, ui: &mut egui::Ui, devices: &Devices) {
        let snapshot = devices.snapshot();
        let Some(backend) = snapshot.backend else {
            ui.heading("Devices");
            ui.label(
                "This system has no controller backend in anygamepad yet: Linux works today, \
                 Windows (roadmap task C5) and macOS (C6) come next.",
            );
            return;
        };
        if self
            .selected
            .is_none_or(|id| snapshot.devices.iter().all(|d| d.id != id))
        {
            self.selected = snapshot.devices.first().map(|d| d.id);
        }
        egui::SidePanel::left("device-list")
            .resizable(true)
            .default_width(240.0)
            .show_inside(ui, |ui| self.list(ui, &snapshot, backend));
        egui::CentralPanel::default().show_inside(ui, |ui| {
            match snapshot
                .devices
                .iter()
                .find(|d| Some(d.id) == self.selected)
            {
                Some(device) => {
                    egui::ScrollArea::vertical().show(ui, |ui| detail(ui, device, devices));
                }
                None => {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.heading("No controller");
                        ui.label("Connect a controller; it shows up here.");
                    });
                }
            }
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot, backend: &str) {
        ui.heading("Controllers");
        ui.label(
            egui::RichText::new(format!("Backend: {backend}"))
                .small()
                .weak(),
        );
        ui.add_space(6.0);
        for device in &snapshot.devices {
            let identity = &device.info.identity;
            let name = identity
                .product_name
                .as_deref()
                .unwrap_or("Unnamed controller");
            let ids = match (identity.vendor_id, identity.product_id) {
                (Some(v), Some(p)) => format!("{v:04x}:{p:04x} · {}", identity.transport),
                _ => identity.transport.to_string(),
            };
            let selected = Some(device.id) == self.selected;
            let response = ui.selectable_label(selected, format!("{name}\n{ids}"));
            if response.clicked() {
                self.selected = Some(device.id);
            }
        }
        if snapshot.inaccessible > 0 {
            ui.add_space(8.0);
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "{} controller(s) found without permission to read them. \
                     See docs/linux-permissions.md.",
                    snapshot.inaccessible
                ),
            );
        }
    }
}

fn detail(ui: &mut egui::Ui, device: &DeviceView, devices: &Devices) {
    let identity = &device.info.identity;
    ui.heading(
        identity
            .product_name
            .as_deref()
            .unwrap_or("Unnamed controller"),
    );
    if let Some(error) = &device.error {
        ui.colored_label(ui.visuals().error_fg_color, error);
    }
    egui::Grid::new("identity")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String| {
                ui.label(egui::RichText::new(label).weak());
                ui.label(value);
                ui.end_row();
            };
            if let (Some(v), Some(p)) = (identity.vendor_id, identity.product_id) {
                row("VID:PID", format!("{v:04x}:{p:04x}"));
            }
            row("Connection", identity.transport.to_string());
            if let Some(driver) = &identity.driver {
                row("Driver", driver.clone());
            }
            row(
                "Mapping",
                match &device.mapping {
                    MappingSource::Profile(name) => format!("profile \u{201c}{name}\u{201d}"),
                    MappingSource::LinuxSpec => "Linux gamepad specification".into(),
                    MappingSource::Canonical => "canonical from the backend".into(),
                    MappingSource::None => "none: raw values only".into(),
                },
            );
            row("Elements", summary(&device.info.layout));
            let power = device.state.power;
            if power.state != PowerState::Unknown {
                let percent = power.percent.map(|p| format!(" {p} %")).unwrap_or_default();
                row("Battery", format!("{}{percent}", power.state));
            }
        });
    if can_rumble(device.info.outputs) {
        ui.horizontal(|ui| {
            let rumble = |strength: f32| {
                OutputCommand::Rumble(Rumble {
                    low_frequency: strength,
                    high_frequency: strength,
                })
            };
            if ui.button("Rumble").clicked() {
                devices.send(Command::Send(device.id, rumble(0.6)));
            }
            if ui.button("Stop").clicked() {
                devices.send(Command::Send(device.id, rumble(0.0)));
            }
        });
    }
    ui.separator();
    ui.columns(2, |columns| {
        columns[0].strong("Canonical");
        canonical(&mut columns[0], &device.state);
        columns[1].strong("Raw");
        raw(&mut columns[1], device);
    });
}

/// The canonical state: sticks, triggers and the buttons pressed.
fn canonical(ui: &mut egui::Ui, state: &GamepadState) {
    ui.horizontal(|ui| {
        stick(
            ui,
            "Left",
            state.left_stick(),
            state.pressed(Button::LeftStick),
        );
        stick(
            ui,
            "Right",
            state.right_stick(),
            state.pressed(Button::RightStick),
        );
    });
    for (label, axis) in [
        ("Left trigger", Axis::LeftTrigger),
        ("Right trigger", Axis::RightTrigger),
    ] {
        let value = state.axis(axis);
        ui.add(egui::ProgressBar::new(value).text(format!("{label} {value:.2}")));
    }
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        for button in Button::STANDARD {
            let pressed = state.pressed(button);
            let text = egui::RichText::new(button.as_str()).small();
            let text = if pressed {
                text.strong().color(ui.visuals().selection.stroke.color)
            } else {
                text.weak()
            };
            ui.label(text);
        }
    });
}

/// A stick: its position in a circle; filled when clicked.
fn stick(ui: &mut egui::Ui, label: &str, position: Stick, clicked: bool) {
    ui.vertical(|ui| {
        let size = 96.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
        let painter = ui.painter_at(rect);
        let center = rect.center();
        let radius = size / 2.0 - 4.0;
        let visuals = ui.visuals();
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0_f32, visuals.weak_text_color()),
        );
        let dot = Pos2::new(
            center.x + position.x * radius,
            center.y - position.y * radius,
        );
        let color = visuals.selection.stroke.color;
        if clicked {
            painter.circle_filled(dot, 8.0, color);
        } else {
            painter.circle_stroke(dot, 8.0, Stroke::new(2.0_f32, color));
        }
        painter.line_segment(
            [center, dot],
            Stroke::new(1.0_f32, color.gamma_multiply(0.5)),
        );
        ui.label(format!("{label} {:+.2} {:+.2}", position.x, position.y));
    });
}

/// Raw values, by source.
fn raw(ui: &mut egui::Ui, device: &DeviceView) {
    let layout = &device.info.layout;
    let raw = &device.raw;
    egui::Grid::new("raw")
        .num_columns(2)
        .striped(true)
        .show(ui, |ui| {
            for (source, value) in layout.buttons.iter().zip(&raw.buttons) {
                ui.label(source_name(source));
                ui.label(if *value > 0.0 {
                    format!("{value:.2}")
                } else {
                    "·".into()
                });
                ui.end_row();
            }
            for (info, value) in layout.axes.iter().zip(&raw.axes) {
                ui.label(source_name(&info.source));
                let span = info.max - info.min;
                let t = if span > 0.0 {
                    (value - info.min) / span
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(t.clamp(0.0, 1.0))
                        .desired_width(140.0)
                        .text(if value.fract() == 0.0 {
                            format!("{value}")
                        } else {
                            format!("{value:.3}")
                        }),
                );
                ui.end_row();
            }
            for (source, hat) in layout.hats.iter().zip(&raw.hats) {
                ui.label(source_name(source));
                ui.label(hat_text(*hat));
                ui.end_row();
            }
        });
    if let Some(sample) = raw.motion.last() {
        ui.add_space(6.0);
        if let Some(g) = sample.gyro {
            ui.label(format!("Gyro {:+.2} {:+.2} {:+.2} rad/s", g.x, g.y, g.z));
        }
        if let Some(a) = sample.accel {
            ui.label(format!("Accel {:+.2} {:+.2} {:+.2} m/s²", a.x, a.y, a.z));
        }
    }
    let touches: Vec<_> = raw.touches.iter().flatten().collect();
    if !layout.touchpads.is_empty() {
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(192.0, 96.0), Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
            egui::StrokeKind::Inside,
        );
        for touch in touches {
            let at = Pos2::new(
                rect.left() + touch.x * rect.width(),
                rect.top() + touch.y * rect.height(),
            );
            painter.circle_filled(at, 6.0, Color32::from_rgb(0x00, 0xd0, 0xeb));
        }
    }
}

fn source_name(source: &RawSource) -> String {
    source.to_string()
}

fn hat_text(hat: Hat) -> String {
    let mut text = String::new();
    for (bit, name) in [
        (Hat::UP, "up"),
        (Hat::RIGHT, "right"),
        (Hat::DOWN, "down"),
        (Hat::LEFT, "left"),
    ] {
        if hat.has(bit) {
            if !text.is_empty() {
                text.push('+');
            }
            text.push_str(name);
        }
    }
    if text.is_empty() { "·".into() } else { text }
}
