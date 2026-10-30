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
