# 0001 — Canonical model naming

- Status: accepted (2026-10-10)
- Date: 2026-10-10
- Roadmap: F3 (ADR 1); used by C1

## Context

The same physical button has different labels per family: the bottom face
button is `A` on Xbox, `Cross` on PlayStation and `B` on Nintendo. Games need
stable input (jump is always the bottom button) and also the right glyph for
prompts. SDL3, the Linux gamepad specification, gilrs and the W3C standard
mapping all name face buttons by position
([research](../research/08-provider-combination.md#canonical-model-consequences-c1)).

Conventions also differ for axes: SDL3 and the W3C standard mapping report up
as negative Y; XInput, gilrs (and therefore Bevy) report up as positive Y.

## Decision

1. **Positional names** for every control. Face buttons: `South`, `East`,
   `West`, `North`.
2. A separate **label layer**: each device reports a `FaceLayout`
   (`Xbox`, `PlayStation`, `Nintendo`, `Generic`, ...) and a `label(button)`
   query returns `A/B/X/Y`, `Cross/Circle/Square/Triangle`, Nintendo
   `B/A/Y/X`, or `Unknown`. Games use positions for logic and labels only for
   prompts.
3. Control names (C1 defines the full list):
   - Buttons: `South`, `East`, `West`, `North`, `DPadUp`, `DPadDown`,
     `DPadLeft`, `DPadRight`, `LeftShoulder`, `RightShoulder`,
     `LeftTrigger`, `RightTrigger` (digital view of the analog triggers),
     `LeftStick`, `RightStick` (clicks), `Select`, `Start`, `Home`, `Share`,
     `Capture`, `Mute`, `LeftPaddle1`, `LeftPaddle2`, `RightPaddle1`,
     `RightPaddle2`, `TouchpadClick`, plus `Extra(n)` for anything else.
     Unlike SDL's `MISC1`, share, capture and mute are distinct buttons.
   - Axes: `LeftStickX`, `LeftStickY`, `RightStickX`, `RightStickY` in
     `[-1.0, 1.0]`; `LeftTrigger`, `RightTrigger` in `[0.0, 1.0]`.
4. **Axis direction**: X positive = right, **Y positive = up**. Reason: the
   library's users are Rust games, and gilrs/Bevy and XInput already use
   positive-up; it is also the mathematical convention. The web bindings
   expose the same values (documented, since the browser API uses the
   opposite sign).
5. **Physical units**: gyroscope in rad/s, accelerometer in m/s² (SI), with a
   right-handed device frame defined in C1; touch points normalized to
   `[0.0, 1.0]` with the touch surface size available.
6. Every control and output (rumble, HD rumble, lightbar, player LEDs,
   adaptive triggers) belongs to a **capability set** a game can query.

## Alternatives

- Label-based names (`A/B/X/Y`): ambiguous across families; rejected by every
  modern API listed above.
- Up = negative Y (SDL3, W3C): consistent with screen coordinates, but
  different from the Rust ecosystem the library targets.

## Consequences

- Profiles (D1) and the mapping engine (C3) target positional names only.
- The label table is data in the profile database, so new families need no
  code change.
- The web tester shows both the position and the label of each control.
