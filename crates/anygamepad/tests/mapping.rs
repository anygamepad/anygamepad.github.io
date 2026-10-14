//! The mapping engine with real profiles: the curated Gamepad API standard
//! profile and SDL GameControllerDB entries (D2).

use anygamepad::db::sdl::{self, VENDORED_ATTRIBUTION};
use anygamepad::db::{Indices, Input, Mapping};
use anygamepad::mapping::Mapper;
use anygamepad::model::raw::{Hat, RawAxisInfo, RawLayout, RawSource, RawState};
use anygamepad::model::{Axis, Button, Capabilities, GamepadState};

const SDL_DB: &str = include_str!("../../../data/sdl-gamecontrollerdb/gamecontrollerdb.txt");

/// A layout that lists its elements by index, as the Gamepad API does and
/// as SDL's joystick indices count them.
fn indexed_layout(buttons: u16, axes: u16, hats: u16, (min, max): (f32, f32)) -> RawLayout {
    RawLayout {
        buttons: (0..buttons).map(RawSource::Index).collect(),
        axes: (0..axes)
            .map(|index| RawAxisInfo {
                source: RawSource::Index(index),
                min,
                max,
            })
            .collect(),
        hats: (0..hats).map(RawSource::Index).collect(),
        ..RawLayout::default()
    }
}

/// SDL's joystick axis range.
const SDL_RANGE: (f32, f32) = (-32768.0, 32767.0);

/// The mapping of one SDL GameControllerDB line.
fn sdl_mapping(line: &str) -> Mapping {
    let profile = sdl::import_line(line, VENDORED_ATTRIBUTION)
        .unwrap()
        .unwrap();
    assert_eq!(profile.mappings.len(), 1);
    profile.mappings[0].clone()
}

fn map(mapper: &Mapper, raw: &RawState) -> GamepadState {
    let mut state = GamepadState::new();
    mapper.apply(raw, &mut state);
    state
}

#[cfg(feature = "web")]
#[test]
fn standard_profile_agrees_with_the_standard_table() {
    use anygamepad::db::Profile;
    use anygamepad::mapping::PRESS_THRESHOLD;
    use anygamepad::model::OutputSet;
    use anygamepad::web::standard;

    let profile = Profile::from_toml(include_str!(
        "../../../data/profiles/gamepad-api-standard.toml"
    ))
    .unwrap();
    // The 17 buttons and 4 axes of the standard layout, as WebProvider
    // lists them.
    let layout = indexed_layout(17, 4, 0, (-1.0, 1.0));
    let mapper = Mapper::new(&profile.mappings[0], &layout);
    assert!(mapper.unresolved().is_empty());
    assert_eq!(
        *mapper.capabilities(),
        standard::capabilities(17, 4, OutputSet::EMPTY)
    );

    let special = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.5,
        -0.5,
        PRESS_THRESHOLD,
        PRESS_THRESHOLD - f32::EPSILON,
        0.3,
        f32::NAN,
        2.0,
        -2.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
    ];
    // A fixed linear congruential generator: the same values on every run.
    let mut seed = 0x2545_f491_u32;
    let mut next = || {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        match seed >> 28 {
            0..=5 => special[(seed >> 8) as usize % special.len()],
            _ => (seed >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0,
        }
    };
    for _ in 0..2000 {
        let mut raw = RawState::for_layout(&layout);
        raw.buttons.iter_mut().for_each(|value| *value = next());
        raw.axes.iter_mut().for_each(|value| *value = next());
        let mut expected = GamepadState::new();
        standard::map(&raw, &mut expected);
        assert_eq!(map(&mapper, &raw), expected, "{raw:?}");
    }
}

#[test]
fn sdl_linux_xbox_360_pad() {
    // Hat d-pad, Y axes growing downwards, triggers on whole axes resting
    // at the minimum.
    let mapping = sdl_mapping(
        "030000006f0e00001302000000010000,Afterglow Xbox 360 Controller,a:b0,b:b1,\
         back:b6,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b8,leftshoulder:b4,\
         leftstick:b9,lefttrigger:a2,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b10,\
         righttrigger:a5,rightx:a3,righty:a4,start:b7,x:b2,y:b3,platform:Linux,",
    );
    let layout = indexed_layout(11, 6, 1, SDL_RANGE);
    let mapper = Mapper::new(&mapping, &layout);
    assert!(mapper.unresolved().is_empty());
    // The trigger buttons follow the trigger axes: every standard control.
    assert_eq!(*mapper.capabilities(), Capabilities::standard());

    // Sticks centred (halfway through SDL's range, which 0 misses by
    // 1/65535) and triggers released.
    let mut raw = RawState::for_layout(&layout);
    raw.axes = vec![-0.5, -0.5, -32768.0, -0.5, -0.5, -32768.0];
    assert_eq!(map(&mapper, &raw), GamepadState::new());

    raw.buttons[0] = 1.0; // a
    raw.buttons[7] = 1.0; // start
    raw.hats[0] = Hat(Hat::UP | Hat::RIGHT);
    raw.axes = vec![32767.0, -32768.0, 32767.0, -0.5, 32767.0, -32768.0];
    let state = map(&mapper, &raw);
    let pressed: Vec<Button> = state.buttons.iter().collect();
    assert_eq!(
        pressed,
        [
            Button::South,
            Button::DPadUp,
            Button::DPadRight,
            Button::LeftTrigger,
            Button::Start
        ]
    );
    assert_eq!(state.axis(Axis::LeftStickX), 1.0);
    // Up on SDL's axis (its minimum) is up in the canonical model.
    assert_eq!(state.axis(Axis::LeftStickY), 1.0);
    assert_eq!(state.axis(Axis::RightStickY), -1.0);
    assert_eq!(state.axis(Axis::LeftTrigger), 1.0);
    assert_eq!(state.axis(Axis::RightTrigger), 0.0);
}

#[test]
fn sdl_combined_trigger_axis_and_button_triggers() {
    // Both triggers on axis 3: left towards the maximum, right towards the
    // minimum.
    let cyborg = sdl_mapping(
        "03000000a306000022f6000000000000,Cyborg V.3 Rumble,a:b1,b:b2,back:b8,\
         dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,leftshoulder:b4,leftstick:b10,\
         lefttrigger:+a3,leftx:a0,lefty:a1,rightshoulder:b5,rightstick:b11,\
         righttrigger:-a3,rightx:a2,righty:a4,start:b9,x:b0,y:b3,platform:Windows,",
    );
    let layout = indexed_layout(12, 5, 1, SDL_RANGE);
    let mapper = Mapper::new(&cyborg, &layout);
    assert!(mapper.unresolved().is_empty());
    let triggers = |value: f32| {
        let mut raw = RawState::for_layout(&layout);
        raw.axes[3] = value;
        let state = map(&mapper, &raw);
        (
            state.axis(Axis::LeftTrigger),
            state.axis(Axis::RightTrigger),
        )
    };
    assert_eq!(triggers(-0.5), (0.0, 0.0));
    assert_eq!(triggers(32767.0), (1.0, 0.0));
    assert_eq!(triggers(-32768.0), (0.0, 1.0));

    // Digital triggers: SDL maps them as axes driven by buttons.
    let mad_catz = sdl_mapping(
        "03000000380700003847000090040000,Mad Catz Xbox 360 Controller,a:b0,b:b1,back:b8,\
         dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,guide:b10,leftshoulder:b4,\
         lefttrigger:b6,leftx:a0,lefty:a1,rightshoulder:b5,righttrigger:b7,rightx:a2,\
         righty:a3,start:b9,x:b2,y:b3,platform:Linux,",
    );
    let layout = indexed_layout(11, 4, 1, SDL_RANGE);
    let mapper = Mapper::new(&mad_catz, &layout);
    let mut raw = RawState::for_layout(&layout);
    raw.buttons[6] = 1.0;
    let state = map(&mapper, &raw);
    assert_eq!(state.axis(Axis::LeftTrigger), 1.0);
    assert!(state.pressed(Button::LeftTrigger));
    assert!(!state.pressed(Button::RightTrigger));
}

#[test]
fn sdl_halves() {
    // The C buttons of an N64 pad as the right stick's halves.
    let n64 = sdl_mapping(
        "03000000c82d00000290000000000000,8BitDo N64,+rightx:b9,+righty:b3,-rightx:b4,\
         -righty:b8,a:b0,b:b1,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,\
         leftshoulder:b6,lefttrigger:b10,leftx:a0,lefty:a1,rightshoulder:b7,start:b11,\
         platform:Windows,",
    );
    let layout = indexed_layout(12, 2, 1, SDL_RANGE);
    let mapper = Mapper::new(&n64, &layout);
    assert!(mapper.unresolved().is_empty());
    let right_stick = |buttons: &[usize]| {
        let mut raw = RawState::for_layout(&layout);
        for &button in buttons {
            raw.buttons[button] = 1.0;
        }
        let stick = map(&mapper, &raw).right_stick();
        (stick.x, stick.y)
    };
    assert_eq!(right_stick(&[]), (0.0, 0.0));
    // C-up (SDL's -righty) is up, C-down is down.
    assert_eq!(right_stick(&[8]), (0.0, 1.0));
    assert_eq!(right_stick(&[3]), (0.0, -1.0));
    assert_eq!(right_stick(&[4, 9]), (0.0, 0.0));
    assert_eq!(right_stick(&[9, 8]), (1.0, 1.0));

    // A hat as the left stick, axes as the d-pad.
    let gravis = sdl_mapping(
        "030000007d0400000840000000000000,Gravis Destroyer Tilt,+leftx:h0.2,+lefty:h0.4,\
         -leftx:h0.8,-lefty:h0.1,a:b1,b:b2,dpdown:+a1,dpleft:-a0,dpright:+a0,dpup:-a1,\
         leftshoulder:b4,rightshoulder:b5,x:b0,y:b3,platform:Windows,",
    );
    let layout = indexed_layout(6, 2, 1, SDL_RANGE);
    let mapper = Mapper::new(&gravis, &layout);
    assert!(mapper.unresolved().is_empty());
    let mut raw = RawState::for_layout(&layout);
    raw.hats[0] = Hat(Hat::UP | Hat::LEFT);
    raw.axes = vec![32767.0, -32768.0];
    let state = map(&mapper, &raw);
    assert_eq!(state.left_stick().x, -1.0);
    assert_eq!(state.left_stick().y, 1.0);
    let pressed: Vec<Button> = state.buttons.iter().collect();
    assert_eq!(pressed, [Button::DPadUp, Button::DPadRight]);

    // Half an axis driving half a stick axis.
    let ipega = sdl_mapping(
        "03000000491900000304000000000000,Ipega PG9087,+righty:+a5,-righty:-a4,a:b0,b:b1,\
         back:b10,dpdown:h0.4,dpleft:h0.8,dpright:h0.2,dpup:h0.1,leftshoulder:b6,\
         leftstick:b13,lefttrigger:b8,leftx:a0,lefty:a1,rightshoulder:b7,rightstick:b14,\
         righttrigger:b9,rightx:a3,start:b11,x:b3,y:b4,platform:Windows,",
    );
    let layout = indexed_layout(15, 6, 1, SDL_RANGE);
    let mapper = Mapper::new(&ipega, &layout);
    assert!(mapper.unresolved().is_empty());
    let right_y = |a4: f32, a5: f32| {
        let mut raw = RawState::for_layout(&layout);
        raw.axes[4] = a4;
        raw.axes[5] = a5;
        map(&mapper, &raw).axis(Axis::RightStickY)
    };
    // SDL's +righty (down) reads the positive half of axis 5.
    assert_eq!(right_y(-0.5, 32767.0), -1.0);
    assert_eq!(right_y(-32768.0, -0.5), 1.0);
    // Each axis only drives its own half.
    assert_eq!(right_y(32767.0, -32768.0), 0.0);
}

#[test]
fn every_sdl_entry_resolves_against_its_own_size() {
    let mut count = 0;
    for (line, profile) in sdl::import(SDL_DB, VENDORED_ATTRIBUTION) {
        let profile = profile.unwrap_or_else(|error| panic!("line {line}: {error}"));
        assert_eq!(profile.indices, Indices::Sdl);
        let mapping = &profile.mappings[0];
        let inputs = mapping.buttons.values().copied().chain(
            mapping
                .axes
                .values()
                .flat_map(|axis| [axis.input, axis.negative, axis.positive])
                .flatten(),
        );
        // As many elements of each kind as the entry's highest index needs.
        let (mut buttons, mut axes, mut hats) = (0, 0, 0);
        for input in inputs {
            let RawSource::Index(index) = input.source() else {
                panic!("line {line}: SDL entries use indices");
            };
            let needed = match input {
                Input::Button(_) => &mut buttons,
                Input::Axis { .. } => &mut axes,
                Input::Hat { .. } => &mut hats,
            };
            *needed = (*needed).max(index + 1);
        }
        let layout = indexed_layout(buttons, axes, hats, SDL_RANGE);
        let mapper = Mapper::new(mapping, &layout);
        assert!(mapper.unresolved().is_empty(), "line {line}");
        assert_eq!(
            mapper.capabilities().axes.len(),
            mapping.axes.len(),
            "line {line}"
        );
        // A centred state maps without panicking.
        map(&mapper, &RawState::for_layout(&layout));
        count += 1;
    }
    assert!(count > 2000, "{count} entries");
}
