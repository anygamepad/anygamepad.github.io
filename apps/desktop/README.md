# Desktop app

Detects controllers and exposes a virtual controller of a selectable type,
with an always-on-top widget and a collection mode.

Built with egui through eframe ([ADR 0004](../../docs/adr/0004-desktop-ui-toolkit.md)).
So far it is the shell (roadmap task A1): the main window with its pages,
settings, the system tray and the log. Device pages arrive with A2, A3 and
the virtual pad tasks.

```sh
cargo run -p anygamepad-desktop --release
```

The binary is `anygamepad`.

## Where it keeps things

| | Linux | Windows | macOS |
|---|---|---|---|
| Settings (`settings.toml`) | `$XDG_CONFIG_HOME/anygamepad` (`~/.config/anygamepad`) | `%APPDATA%\anygamepad` | `~/Library/Application Support/anygamepad` |
| Log (`anygamepad.log`, restarted at each launch) | `$XDG_STATE_HOME/anygamepad` (`~/.local/state/anygamepad`) | `%LOCALAPPDATA%\anygamepad\logs` | `~/Library/Logs/anygamepad` |

The log never holds serial numbers, Bluetooth addresses or device paths.

## System tray

Linux: StatusNotifierItem over D-Bus (KDE, most desktops; GNOME needs the
AppIndicator extension). Windows and macOS: the native tray. Without a
tray the window stays open, and the tray settings are disabled.

## Runtime libraries on Linux

The window needs `libxkbcommon-x11` (X11) or `libwayland-client` and
`libxkbcommon` (Wayland), and OpenGL. Desktop distributions ship them.

## Icon

`assets/icon-64.rgba` is the logo's mark (`apps/web/public/favicon.svg`)
rendered at 64×64, raw RGBA.

## Widget

Settings, Widget: a small window that shows one controller above other
windows (roadmap task A11), drawn as a controller or compactly, with an
adjustable background and optional click-through (turn it off to drag the
widget). Limits:

- **Wayland**: regular windows cannot keep themselves on top; use the
  window menu's "Always on top" where the desktop has it (KDE, most
  wlroots desktops), or run the app under XWayland
  (`WAYLAND_DISPLAY= anygamepad`). A layer-shell widget is a later step.
- **X11**: the window manager decides; most honour the request.
- **Transparency** needs an OpenGL configuration with alpha and, on X11, a
  compositor; without them the widget is opaque.
- **Exclusive full-screen games** cover every window on every OS: play in
  borderless windowed mode to see the widget.
