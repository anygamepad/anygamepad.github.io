//! Buttons of the canonical gamepad, named by position (ADR 0001).

use core::fmt;
use core::str::FromStr;

use crate::ParseNameError;
use crate::macros::{enum_set, impl_name_serde};

/// Number of extra buttons a gamepad can report beyond the standard ones.
pub const MAX_EXTRA_BUTTONS: usize = 32;

/// A digital control of the canonical gamepad.
///
/// Buttons are named by **position**, never by the label printed on them: the
/// bottom face button is [`Button::South`] whether it says `A` (Xbox), `Cross`
/// (PlayStation) or `B` (Nintendo). Use [`FaceLayout`](crate::FaceLayout) to
/// get the printed label for prompts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Button {
    /// Bottom face button (Xbox `A`, PlayStation `Cross`, Nintendo `B`).
    South,
    /// Right face button (Xbox `B`, PlayStation `Circle`, Nintendo `A`).
    East,
    /// Left face button (Xbox `X`, PlayStation `Square`, Nintendo `Y`).
    West,
    /// Top face button (Xbox `Y`, PlayStation `Triangle`, Nintendo `X`).
    North,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
    /// Left shoulder button (Xbox `LB`, PlayStation `L1`, Nintendo `L`).
    LeftShoulder,
    /// Right shoulder button (Xbox `RB`, PlayStation `R1`, Nintendo `R`).
    RightShoulder,
    /// Digital view of the left trigger (Xbox `LT`, PlayStation `L2`,
    /// Nintendo `ZL`). The analog value is [`Axis::LeftTrigger`](crate::Axis::LeftTrigger).
    LeftTrigger,
    /// Digital view of the right trigger (Xbox `RT`, PlayStation `R2`,
    /// Nintendo `ZR`). The analog value is [`Axis::RightTrigger`](crate::Axis::RightTrigger).
    RightTrigger,
    /// Left stick click (`L3`).
    LeftStick,
    /// Right stick click (`R3`).
    RightStick,
    /// Left menu button: Xbox `Back`/`View`, DualShock 4 `Share`, DualSense
    /// `Create`, Nintendo `-`.
    Select,
    /// Right menu button: Xbox `Start`/`Menu`, PlayStation `Options`,
    /// Nintendo `+`.
    Start,
    /// Central system button: Xbox `Guide`, PlayStation `PS`, Nintendo `Home`.
    Home,
    /// Dedicated share button of the Xbox Series controllers (not the
    /// DualShock 4 `Share` button, which is [`Button::Select`]).
    Share,
    /// Capture button of Nintendo controllers.
    Capture,
    /// Microphone mute button (DualSense).
    Mute,
    /// Upper left back paddle (Xbox Elite `P3`).
    LeftPaddle1,
    /// Lower left back paddle (Xbox Elite `P4`).
    LeftPaddle2,
    /// Upper right back paddle (Xbox Elite `P1`).
    RightPaddle1,
    /// Lower right back paddle (Xbox Elite `P2`).
    RightPaddle2,
    /// Touchpad click (DualShock 4, DualSense).
    TouchpadClick,
    /// A button that has no standard position; see [`ExtraButton`].
    Extra(ExtraButton),
}

/// Index of a non-standard button, below [`MAX_EXTRA_BUTTONS`].
///
/// Profiles decide which physical control each extra index stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtraButton(u8);

impl ExtraButton {
    /// The extra button with this index, if it is below [`MAX_EXTRA_BUTTONS`].
    pub const fn new(index: u8) -> Option<Self> {
        if (index as usize) < MAX_EXTRA_BUTTONS {
            Some(Self(index))
        } else {
            None
        }
    }

    /// The index of this extra button.
    pub const fn index(self) -> u8 {
        self.0
    }
}

/// Bit of the first extra button in a [`ButtonSet`].
const EXTRA_BIT: u32 = 32;

const EXTRA_NAMES: [&str; MAX_EXTRA_BUTTONS] = [
    "extra_0", "extra_1", "extra_2", "extra_3", "extra_4", "extra_5", "extra_6", "extra_7",
    "extra_8", "extra_9", "extra_10", "extra_11", "extra_12", "extra_13", "extra_14", "extra_15",
    "extra_16", "extra_17", "extra_18", "extra_19", "extra_20", "extra_21", "extra_22", "extra_23",
    "extra_24", "extra_25", "extra_26", "extra_27", "extra_28", "extra_29", "extra_30", "extra_31",
];

impl Button {
    /// Every button with a standard position (all but [`Button::Extra`]).
    pub const STANDARD: [Self; 25] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::DPadUp,
        Self::DPadDown,
        Self::DPadLeft,
        Self::DPadRight,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::LeftStick,
        Self::RightStick,
        Self::Select,
        Self::Start,
        Self::Home,
        Self::Share,
        Self::Capture,
        Self::Mute,
        Self::LeftPaddle1,
        Self::LeftPaddle2,
        Self::RightPaddle1,
        Self::RightPaddle2,
        Self::TouchpadClick,
    ];

    /// The extra button with this index, if it is below [`MAX_EXTRA_BUTTONS`].
    pub const fn extra(index: u8) -> Option<Self> {
        match ExtraButton::new(index) {
            Some(extra) => Some(Self::Extra(extra)),
            None => None,
        }
    }

    /// Whether this is one of the four face buttons.
    pub const fn is_face(self) -> bool {
        matches!(self, Self::South | Self::East | Self::West | Self::North)
    }

    /// Whether this is one of the four d-pad directions.
    pub const fn is_dpad(self) -> bool {
        matches!(
            self,
            Self::DPadUp | Self::DPadDown | Self::DPadLeft | Self::DPadRight
        )
    }

    /// Stable snake_case name, used in profiles, reports and bindings
    /// (`south`, `dpad_up`, `left_paddle_1`, `extra_3`, ...).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::DPadUp => "dpad_up",
            Self::DPadDown => "dpad_down",
            Self::DPadLeft => "dpad_left",
            Self::DPadRight => "dpad_right",
            Self::LeftShoulder => "left_shoulder",
            Self::RightShoulder => "right_shoulder",
            Self::LeftTrigger => "left_trigger",
            Self::RightTrigger => "right_trigger",
            Self::LeftStick => "left_stick",
            Self::RightStick => "right_stick",
            Self::Select => "select",
            Self::Start => "start",
            Self::Home => "home",
            Self::Share => "share",
            Self::Capture => "capture",
            Self::Mute => "mute",
            Self::LeftPaddle1 => "left_paddle_1",
            Self::LeftPaddle2 => "left_paddle_2",
            Self::RightPaddle1 => "right_paddle_1",
            Self::RightPaddle2 => "right_paddle_2",
            Self::TouchpadClick => "touchpad_click",
            Self::Extra(extra) => EXTRA_NAMES[extra.0 as usize],
        }
    }

    /// Bit in a [`ButtonSet`]: standard buttons use bits 0-24, extra buttons
    /// bits 32-63.
    pub(crate) const fn bit(self) -> u32 {
        // Same order as `STANDARD` (checked by the tests).
        match self {
            Self::South => 0,
            Self::East => 1,
            Self::West => 2,
            Self::North => 3,
            Self::DPadUp => 4,
            Self::DPadDown => 5,
            Self::DPadLeft => 6,
            Self::DPadRight => 7,
            Self::LeftShoulder => 8,
            Self::RightShoulder => 9,
            Self::LeftTrigger => 10,
            Self::RightTrigger => 11,
            Self::LeftStick => 12,
            Self::RightStick => 13,
            Self::Select => 14,
            Self::Start => 15,
            Self::Home => 16,
            Self::Share => 17,
            Self::Capture => 18,
            Self::Mute => 19,
            Self::LeftPaddle1 => 20,
            Self::LeftPaddle2 => 21,
            Self::RightPaddle1 => 22,
            Self::RightPaddle2 => 23,
            Self::TouchpadClick => 24,
            Self::Extra(extra) => EXTRA_BIT + extra.0 as u32,
        }
    }

    pub(crate) const fn from_bit(bit: u32) -> Option<Self> {
        if (bit as usize) < Self::STANDARD.len() {
            Some(Self::STANDARD[bit as usize])
        } else if bit >= EXTRA_BIT && bit < 64 {
            Some(Self::Extra(ExtraButton((bit - EXTRA_BIT) as u8)))
        } else {
            None
        }
    }
}

impl fmt::Display for Button {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Button {
    type Err = ParseNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let error = || ParseNameError::new("button", s);
        if let Some(index) = s.strip_prefix("extra_") {
            // Only the canonical spelling: no sign, no leading zeros.
            let button = index.parse().ok().and_then(Self::extra).ok_or_else(error)?;
            return if button.as_str() == s {
                Ok(button)
            } else {
                Err(error())
            };
        }
        Self::STANDARD
            .into_iter()
            .find(|button| button.as_str() == s)
            .ok_or_else(error)
    }
}

impl_name_serde!(Button, "button");

#[cfg(feature = "schemars")]
impl schemars::JsonSchema for Button {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Button".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let standard: Vec<&str> = Self::STANDARD.iter().map(|b| b.as_str()).collect();
        schemars::json_schema!({
            "type": "string",
            "anyOf": [
                { "enum": standard },
                { "pattern": "^extra_([0-9]|[12][0-9]|3[01])$" },
            ],
        })
    }
}

enum_set! {
    /// A set of [`Button`]s: the buttons pressed in a state, or the buttons a
    /// device has.
    pub struct ButtonSet(u64): Button;
}

impl ButtonSet {
    /// Every standard button (no extra buttons).
    pub const STANDARD: Self = {
        let mut set = Self::EMPTY;
        let mut i = 0;
        while i < Button::STANDARD.len() {
            set = set.with(Button::STANDARD[i]);
            i += 1;
        }
        set
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_are_unique_and_round_trip() {
        let mut seen = ButtonSet::EMPTY;
        let extras = (0..MAX_EXTRA_BUTTONS as u8).map(|i| Button::extra(i).unwrap());
        for button in Button::STANDARD.into_iter().chain(extras) {
            assert!(seen.insert(button), "duplicate bit for {button}");
            assert_eq!(Button::from_bit(button.bit()), Some(button));
        }
        assert_eq!(seen, ButtonSet::ALL);
        assert_eq!(ButtonSet::ALL.len(), 25 + MAX_EXTRA_BUTTONS);
        assert_eq!(ButtonSet::STANDARD.len(), 25);
    }

    #[test]
    fn names_round_trip() {
        for bit in 0..64 {
            if let Some(button) = Button::from_bit(bit) {
                assert_eq!(button.as_str().parse::<Button>(), Ok(button));
            }
        }
    }

    #[test]
    fn rejects_non_canonical_extra_names() {
        for name in [
            "extra_32", "extra_01", "extra_+1", "extra_", "extra_x", "South",
        ] {
            let error = name.parse::<Button>().unwrap_err();
            assert_eq!(error.name(), name);
        }
    }

    #[test]
    fn extra_range() {
        assert!(Button::extra(31).is_some());
        assert!(Button::extra(32).is_none());
        assert_eq!(ExtraButton::new(5).map(ExtraButton::index), Some(5));
    }

    #[test]
    fn set_operations() {
        let mut set = ButtonSet::from([Button::South, Button::Start]);
        assert!(set.contains(Button::South));
        assert!(!set.insert(Button::South));
        assert!(set.remove(Button::Start));
        assert!(!set.remove(Button::Start));
        set.set(Button::extra(3).unwrap(), true);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            [Button::South, Button::extra(3).unwrap()]
        );
        assert!(set.is_subset(ButtonSet::ALL));
        assert_eq!(format!("{set:?}"), "{South, Extra(ExtraButton(3))}");
    }
}
