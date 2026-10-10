# 0002 — Input provider strategy

- Status: accepted (2026-10-10)
- Date: 2026-10-10
- Roadmap: F3 (ADR 2); used by C2, C4-C6, C11-C14

## Context

No single input stack covers every device on every platform: our native
backends give full control, SDL3 has the widest driver coverage, gilrs is the
pure-Rust option, and the browser needs its own backend. Using several at once
risks duplicates and competing handshakes. Research:
[08-provider-combination.md](../research/08-provider-combination.md).

## Decision

1. **Provider trait** (defined in C2): `enumerate`, hotplug events, `open`,
   `poll` raw state, outputs, and `identity()` of each device seen. Polling is
   synchronous and driven by the caller's frame loop (it fits games and
   `requestAnimationFrame`); providers that need it run their own background
   thread for hotplug or reads.
2. **Cargo features**:

   | Feature | Default | Content |
   |---------|---------|---------|
   | `native` | yes (non-wasm) | Linux evdev + hidraw, Windows Raw Input/HID + XInput (+ WGI later), macOS IOKit HID |
   | `web` | automatic on `wasm32` | Gamepad API, WebHID (opt-in at runtime) |
   | `sdl3` | no | SDL3 through `sdl3-sys` (C dependency, zlib) |
   | `gilrs` | no | Pure-Rust fallback |

3. **Arbitration** follows SDL's internal model: providers are ordered, and a
   provider claims a device by identity before lower ones open it. Default
   order: native protocol driver > SDL3 HIDAPI driver > native generic >
   SDL3 generic > gilrs. Profiles can name the preferred provider per device.
   Nodes the OS splits a controller into are grouped into one device first.
4. **SDL coexistence**: devices owned by another provider go to
   `SDL_HIDAPI_IGNORE_DEVICES` and `SDL_GAMECONTROLLER_IGNORE_DEVICES`;
   `SDL_JOYSTICK_ENHANCED_REPORTS` defaults to `auto`.
5. **No double handshakes**: on Linux, when `hid-nintendo` or
   `hid-playstation` is bound, the native provider reads evdev (and hidraw
   read-only) instead of running our protocol driver.
6. **Steam Input**: Steam virtual pads (VID `0x28de`,
   `SteamVirtualGamepadInfo`) are mapped back to the real device or skipped.
7. **Observe-only mode** for collection: every provider opens every device
   without claims and without output reports; disagreements are recorded.
8. HID access in the native provider uses the `hidapi` crate (`linux-native`
   pure Rust hidraw backend on Linux, native backends on Windows and macOS,
   `macos-shared-device` for non-exclusive access). Reason: sync API matches
   the poll model; most used HID crate. Protocol drivers (C7) are sans-IO,
   so switching later is local.

## Alternatives

- SDL3 only: no web support (SDL3 does not target `wasm32-unknown-unknown`),
  C dependency for every game, and SDL's own profiles instead of ours.
- gilrs only: no motion, touchpad, LEDs or raw reports.
- Native only: loses SDL's long tail of HIDAPI drivers.
- `async-hid`: needs an async runtime in every game; not justified for
  polling.

## Consequences

- Games get native input with no C dependency by default; `sdl3` widens
  device coverage at the cost of linking SDL.
- C12 needs correlation keys (VID, PID, bus, OS device path/instance ID) in
  every provider's identity.
- Windows GameInput is not used for now (no Rust bindings, needs a
  redistributable installer); re-evaluate after 0.1.
