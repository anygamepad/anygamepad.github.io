//! The system tray icon: show the window, quit.
//!
//! Linux uses the StatusNotifierItem protocol over D-Bus (KDE, GNOME with
//! the AppIndicator extension, most other desktops), Windows and macOS
//! their native tray. Without a tray (no StatusNotifierItem host), the app
//! keeps its window: start minimized and close to tray do nothing.

use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;

/// What the user picked in the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Show and focus the window.
    Show,
    /// Quit the app.
    Quit,
}

/// A running tray icon.
pub struct Tray {
    events: Receiver<TrayEvent>,
    // Dropping the handle removes the icon.
    _handle: platform::Handle,
}

impl Tray {
    /// Shows the tray icon; `None` when the system has no tray. Events wake
    /// `ctx`.
    pub fn spawn(ctx: &egui::Context, icon: &crate::Icon) -> Option<Self> {
        let (sender, events) = channel();
        let waker = Waker {
            sender,
            ctx: ctx.clone(),
        };
        match platform::spawn(waker, icon) {
            Ok(handle) => Some(Self {
                events,
                _handle: handle,
            }),
            Err(error) => {
                log::info!("no system tray: {error}");
                None
            }
        }
    }

    /// The next event, if any.
    pub fn try_recv(&self) -> Option<TrayEvent> {
        self.events.try_recv().ok()
    }
}

/// Sends tray events to the app and wakes it.
#[derive(Clone)]
struct Waker {
    sender: Sender<TrayEvent>,
    ctx: egui::Context,
}

impl Waker {
    fn send(&self, event: TrayEvent) {
        let _ = self.sender.send(event);
        self.ctx.request_repaint();
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use ksni::blocking::TrayMethods;
    use ksni::menu::StandardItem;

    use super::{TrayEvent, Waker};

    pub type Handle = ksni::blocking::Handle<AppTray>;

    pub struct AppTray {
        waker: Waker,
        /// ARGB32, network byte order.
        pixmap: ksni::Icon,
    }

    impl ksni::Tray for AppTray {
        fn id(&self) -> String {
            "anygamepad".into()
        }

        fn title(&self) -> String {
            "AnyGamepad".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            vec![self.pixmap.clone()]
        }

        fn activate(&mut self, _x: i32, _y: i32) {
            self.waker.send(TrayEvent::Show);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            vec![
                StandardItem {
                    label: "Show AnyGamepad".into(),
                    activate: Box::new(|tray: &mut Self| tray.waker.send(TrayEvent::Show)),
                    ..Default::default()
                }
                .into(),
                ksni::MenuItem::Separator,
                StandardItem {
                    label: "Quit".into(),
                    activate: Box::new(|tray: &mut Self| tray.waker.send(TrayEvent::Quit)),
                    ..Default::default()
                }
                .into(),
            ]
        }
    }

    pub fn spawn(waker: Waker, icon: &crate::Icon) -> Result<Handle, String> {
        // RGBA to ARGB.
        let data = icon
            .rgba
            .chunks_exact(4)
            .flat_map(|p| [p[3], p[0], p[1], p[2]])
            .collect();
        let pixmap = ksni::Icon {
            width: icon.size as i32,
            height: icon.size as i32,
            data,
        };
        AppTray { waker, pixmap }.spawn().map_err(|e| e.to_string())
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod platform {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{TrayEvent, Waker};

    pub type Handle = TrayIcon;

    pub fn spawn(waker: Waker, icon: &crate::Icon) -> Result<Handle, String> {
        let show = MenuItem::new("Show AnyGamepad", true, None);
        let quit = MenuItem::new("Quit", true, None);
        let menu = Menu::new();
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit])
            .map_err(|e| e.to_string())?;
        let (show_id, quit_id) = (show.id().clone(), quit.id().clone());
        let menu_waker = waker.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if event.id == show_id {
                menu_waker.send(TrayEvent::Show);
            } else if event.id == quit_id {
                menu_waker.send(TrayEvent::Quit);
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::DoubleClick { .. } = event {
                waker.send(TrayEvent::Show);
            }
        }));
        let icon =
            Icon::from_rgba(icon.rgba.to_vec(), icon.size, icon.size).map_err(|e| e.to_string())?;
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("AnyGamepad")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod platform {
    use super::Waker;

    pub type Handle = ();

    pub fn spawn(_waker: Waker, _icon: &crate::Icon) -> Result<Handle, String> {
        Err("not supported on this OS".into())
    }
}
