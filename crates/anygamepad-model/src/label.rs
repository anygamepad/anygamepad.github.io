//! Label layer: what is printed on the face buttons (ADR 0001).

use crate::Button;
use crate::macros::named_enum;

named_enum! {
    /// A label printed on a face button, for prompts and glyphs.
    pub enum ButtonLabel: "button label" {
        /// Label not known.
        Unknown = "unknown",
        /// `A`.
        A = "a",
        /// `B`.
        B = "b",
        /// `X`.
        X = "x",
        /// `Y`.
        Y = "y",
        /// PlayStation cross.
        Cross = "cross",
        /// PlayStation circle.
        Circle = "circle",
        /// PlayStation square.
        Square = "square",
        /// PlayStation triangle.
        Triangle = "triangle",
    }
}

/// Labels printed on the four face buttons of a device.
///
/// Game logic uses positions ([`Button::South`], ...); prompts use
/// [`FaceLayout::label`]. The layout of each device comes from its profile, so
/// a new family with known labels needs no code change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "tsify", derive(tsify::Tsify))]
pub struct FaceLayout {
    /// Label of [`Button::South`].
    pub south: ButtonLabel,
    /// Label of [`Button::East`].
    pub east: ButtonLabel,
    /// Label of [`Button::West`].
    pub west: ButtonLabel,
    /// Label of [`Button::North`].
    pub north: ButtonLabel,
}

impl FaceLayout {
    /// Xbox and most PC pads: `A` south, `B` east, `X` west, `Y` north.
    pub const XBOX: Self = Self::new(
        ButtonLabel::A,
        ButtonLabel::B,
        ButtonLabel::X,
        ButtonLabel::Y,
    );

    /// PlayStation: cross south, circle east, square west, triangle north.
    pub const PLAYSTATION: Self = Self::new(
        ButtonLabel::Cross,
        ButtonLabel::Circle,
        ButtonLabel::Square,
        ButtonLabel::Triangle,
    );

    /// Nintendo: `B` south, `A` east, `Y` west, `X` north.
    pub const NINTENDO: Self = Self::new(
        ButtonLabel::B,
        ButtonLabel::A,
        ButtonLabel::Y,
        ButtonLabel::X,
    );

    /// Labels not known.
    pub const UNKNOWN: Self = Self::new(
        ButtonLabel::Unknown,
        ButtonLabel::Unknown,
        ButtonLabel::Unknown,
        ButtonLabel::Unknown,
    );

    /// A layout from the labels of the south, east, west and north buttons.
    pub const fn new(
        south: ButtonLabel,
        east: ButtonLabel,
        west: ButtonLabel,
        north: ButtonLabel,
    ) -> Self {
        Self {
            south,
            east,
            west,
            north,
        }
    }

    /// Label printed on `button`; [`ButtonLabel::Unknown`] for buttons that
    /// are not face buttons.
    pub const fn label(&self, button: Button) -> ButtonLabel {
        match button {
            Button::South => self.south,
            Button::East => self.east,
            Button::West => self.west,
            Button::North => self.north,
            _ => ButtonLabel::Unknown,
        }
    }

    /// Face button that carries `label`, if any.
    pub fn button(&self, label: ButtonLabel) -> Option<Button> {
        if label == ButtonLabel::Unknown {
            return None;
        }
        [Button::South, Button::East, Button::West, Button::North]
            .into_iter()
            .find(|&button| self.label(button) == label)
    }
}

impl Default for FaceLayout {
    fn default() -> Self {
        Self::UNKNOWN
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nintendo_a_is_east() {
        assert_eq!(FaceLayout::NINTENDO.label(Button::East), ButtonLabel::A);
        assert_eq!(FaceLayout::XBOX.label(Button::South), ButtonLabel::A);
        assert_eq!(
            FaceLayout::NINTENDO.button(ButtonLabel::A),
            Some(Button::East)
        );
        assert_eq!(FaceLayout::PLAYSTATION.button(ButtonLabel::A), None);
        assert_eq!(FaceLayout::UNKNOWN.button(ButtonLabel::Unknown), None);
    }

    #[test]
    fn non_face_buttons_have_no_label() {
        assert_eq!(FaceLayout::XBOX.label(Button::Start), ButtonLabel::Unknown);
    }
}
