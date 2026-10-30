//! The main window: a header with the logo, one page per tab: devices
//! (A2), collection (A3), settings, about; virtual pads (A5-A8) are a
//! placeholder.
//!
//! The window repaints only on input or when something wakes it (egui's
//! reactive mode), so an idle app costs next to no CPU.

use std::path::PathBuf;

use eframe::egui;

use crate::collection_page::CollectionPage;
use crate::devices::{Devices, native_provider};
use crate::devices_page::DevicesPage;
use crate::settings::{LogLevel, Settings, Theme};
use crate::tray::{Tray, TrayEvent};
use crate::widget::{Widget, WidgetOptions, WidgetSkin};

/// The pages of the main window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Devices,
    VirtualPad,
    Collection,
    Settings,
    About,
}

impl Tab {
    const ALL: [Self; 5] = [
        Self::Devices,
        Self::VirtualPad,
        Self::Collection,
        Self::Settings,
        Self::About,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Devices => "Devices",
            Self::VirtualPad => "Virtual pad",
            Self::Collection => "Collection",
            Self::Settings => "Settings",
            Self::About => "About",
        }
    }
}

/// The desktop app.
pub struct DesktopApp {
    settings: Settings,
    settings_file: Option<PathBuf>,
    settings_error: Option<String>,
    log_file: Option<PathBuf>,
    tab: Tab,
    tray: Option<Tray>,
    logo: egui::TextureHandle,
    /// The first frame hides the window when it starts minimized.
    first_frame: bool,
    quitting: bool,
    devices: Devices,
    devices_page: DevicesPage,
    collection_page: CollectionPage,
    widget: Widget,
}

impl DesktopApp {
    /// The app, with its settings, where to save them and the log file.
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        settings_file: Option<PathBuf>,
        settings_error: Option<String>,
        log_file: Option<PathBuf>,
        reports: Option<PathBuf>,
    ) -> Self {
        let icon = crate::Icon::app();
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.size as usize, icon.size as usize],
                icon.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        let tray = Tray::spawn(&cc.egui_ctx, &icon);
        apply_theme(&cc.egui_ctx, settings.theme);
        Self {
            settings,
            settings_file,
            settings_error,
            log_file,
            tab: Tab::Devices,
            tray,
            logo,
            first_frame: true,
            devices: Devices::start(native_provider(), cc.egui_ctx.clone()),
            devices_page: DevicesPage::default(),
            collection_page: CollectionPage::new(reports),
            widget: Widget::default(),
            quitting: false,
        }
    }

    fn save_settings(&mut self) {
        let Some(path) = &self.settings_file else {
            return;
        };
        match self.settings.save(path) {
            Ok(()) => {
                self.settings_error = None;
                log::info!("settings saved");
            }
            Err(e) => {
                log::warn!("settings not saved: {e}");
                self.settings_error = Some(format!("Settings could not be saved: {e}"));
            }
        }
    }

    /// Tray events, and the window's close button.
    fn handle_window(&mut self, ctx: &egui::Context) {
        if self.first_frame {
            self.first_frame = false;
            if self.settings.start_minimized && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        while let Some(event) = self.tray.as_ref().and_then(Tray::try_recv) {
            match event {
                TrayEvent::Show => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayEvent::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        let close = ctx.input(|i| i.viewport().close_requested());
        if close && !self.quitting && self.settings.close_to_tray && self.tray.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((self.logo.id(), egui::vec2(28.0, 28.0))));
            // The logo's two-tone wordmark.
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(egui::RichText::new("Any").size(20.0).strong());
            ui.label(
                egui::RichText::new("Gamepad")
                    .size(20.0)
                    .strong()
                    .color(ui.visuals().weak_text_color()),
            );
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_space(16.0);
            for tab in Tab::ALL {
                ui.selectable_value(&mut self.tab, tab, tab.label());
            }
        });
    }

    fn settings_page(&mut self, ui: &mut egui::Ui) {
        let before = self.settings.clone();
        ui.heading("Settings");
        ui.add_space(8.0);
        egui::Grid::new("settings")
            .num_columns(2)
            .spacing([24.0, 10.0])
            .show(ui, |ui| {
                ui.label("Theme");
                ui.horizontal(|ui| {
                    let theme = &mut self.settings.theme;
                    ui.selectable_value(theme, Theme::System, "System");
                    ui.selectable_value(theme, Theme::Light, "Light");
                    ui.selectable_value(theme, Theme::Dark, "Dark");
                });
                ui.end_row();

                ui.label("System tray");
                ui.vertical(|ui| {
                    let tray = self.tray.is_some();
                    ui.add_enabled_ui(tray, |ui| {
                        ui.checkbox(
                            &mut self.settings.start_minimized,
                            "Start hidden in the tray",
                        );
                        ui.checkbox(
                            &mut self.settings.close_to_tray,
                            "Closing the window keeps the app in the tray",
                        );
                    });
                    if !tray {
                        ui.label(
                            egui::RichText::new(
                                "This desktop shows no tray icon, so the window stays open.",
                            )
                            .small()
                            .weak(),
                        );
                    }
                });
                ui.end_row();

                ui.label("Widget");
                ui.vertical(|ui| {
                    ui.checkbox(
                        &mut self.settings.widget,
                        "Show a small window with a controller above other windows",
                    );
                    ui.add_enabled_ui(self.settings.widget, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Controller");
                            let snapshot = self.devices.snapshot();
                            let name = |id| {
                                snapshot
                                    .devices
                                    .iter()
                                    .find(|d| Some(d.id) == id)
                                    .and_then(|d| d.info.identity.product_name.clone())
                                    .unwrap_or_else(|| "The first one".into())
                            };
                            egui::ComboBox::from_id_salt("widget-device")
                                .selected_text(name(self.widget.device))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.widget.device,
                                        None,
                                        "The first one",
                                    );
                                    for device in &snapshot.devices {
                                        ui.selectable_value(
                                            &mut self.widget.device,
                                            Some(device.id),
                                            name(Some(device.id)),
                                        );
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            let skin = &mut self.settings.widget_skin;
                            ui.selectable_value(skin, WidgetSkin::Pad, "Controller");
                            ui.selectable_value(skin, WidgetSkin::Compact, "Compact");
                        });
                        ui.add(
                            egui::Slider::new(&mut self.settings.widget_opacity, 0..=100)
                                .text("Background opacity (%)"),
                        );
                        ui.checkbox(
                            &mut self.settings.widget_click_through,
                            "Let clicks through (turn off to move the widget by dragging it)",
                        );
                    });
                    ui.label(
                        egui::RichText::new(
                            "On Wayland, keep it above other windows from your desktop's window \
                             menu. Games in exclusive full screen cover it: use borderless \
                             windowed mode.",
                        )
                        .small()
                        .weak(),
                    );
                });
                ui.end_row();

                ui.label("Log");
                ui.horizontal(|ui| {
                    let level = &mut self.settings.log_level;
                    ui.selectable_value(level, LogLevel::Error, "Errors");
                    ui.selectable_value(level, LogLevel::Warn, "Warnings");
                    ui.selectable_value(level, LogLevel::Info, "Normal");
                    ui.selectable_value(level, LogLevel::Debug, "Everything");
                });
                ui.end_row();
            });
        if let Some(error) = &self.settings_error {
            ui.add_space(8.0);
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.settings != before {
            if self.settings.theme != before.theme {
                apply_theme(ui.ctx(), self.settings.theme);
            }
            log::set_max_level(self.settings.log_level.filter());
            self.save_settings();
        }
    }

    fn about_page(&self, ui: &mut egui::Ui) {
        ui.heading("About");
        ui.add_space(8.0);
        ui.label(format!("AnyGamepad {}", env!("CARGO_PKG_VERSION")));
        ui.label(format!("anygamepad library {}", anygamepad::VERSION));
        ui.add_space(8.0);
        ui.hyperlink_to("Web tester", "https://anygamepad.github.io");
        ui.hyperlink_to(
            "Source code",
            "https://github.com/anygamepad/anygamepad.github.io",
        );
        ui.hyperlink_to("Privacy", "https://anygamepad.github.io/privacy/");
        ui.add_space(8.0);
        ui.label("Licensed under MIT or Apache-2.0.");
        if let Some(path) = &self.log_file {
            ui.add_space(8.0);
            ui.label(format!("Log file: {}", path.display()));
        }
    }

    fn placeholder(ui: &mut egui::Ui, title: &str, text: &str) {
        ui.heading(title);
        ui.add_space(8.0);
        ui.label(text);
    }
}

fn apply_theme(ctx: &egui::Context, theme: Theme) {
    ctx.set_theme(match theme {
        Theme::System => egui::ThemePreference::System,
        Theme::Light => egui::ThemePreference::Light,
        Theme::Dark => egui::ThemePreference::Dark,
    });
}

impl eframe::App for DesktopApp {
    /// Transparent, so that the widget's window can be; the main window's
    /// panels paint their own background.
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_window(ctx);
        if self.settings.collecting {
            let snapshot = self.devices.snapshot();
            self.collection_page.collect(&self.settings, &snapshot);
        }
        if self.settings.widget {
            let options = WidgetOptions {
                skin: self.settings.widget_skin,
                opacity: self.settings.widget_opacity,
                click_through: self.settings.widget_click_through,
            };
            let snapshot = self.devices.snapshot();
            if !self.widget.show(ctx, &options, &snapshot) {
                self.settings.widget = false;
                self.save_settings();
            }
        }
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::side_top_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| self.header(ui));
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Devices => self.devices_page.show(ui, &self.devices),
            Tab::VirtualPad => Self::placeholder(
                ui,
                "Virtual pad",
                "Present a controller as an Xbox 360 Controller, DualShock 4, DualSense or Switch Pro \
                 Controller to games (roadmap tasks A5 to A8).",
            ),
            Tab::Collection => {
                let snapshot = self.devices.snapshot();
                if self.collection_page.show(ui, &mut self.settings, &snapshot) {
                    self.save_settings();
                }
            }
            Tab::Settings => self.settings_page(ui),
            Tab::About => self.about_page(ui),
        });
    }
}
