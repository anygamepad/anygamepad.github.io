//! Which of several connected pads the person means (roadmap task H10):
//! the test modes ask to press a button on the pad to test, and take the
//! first pad that leaves its rest state.
//!
//! A pad counts as pressed, with the guided wizard's rules
//! ([`wizard`](crate::wizard)), when a button passes half its travel, a hat
//! leaves its centre, or an axis moves half its range away from where it
//! rested. A pad is only considered once it has been seen at rest, so a
//! button held since the pad connected, or a trigger that rests at one end,
//! does not count.
//!
//! Which catalog model, mode and unit that pad is comes from
//! [`db::detect`](crate::db::detect).
//!
//! ```
//! use anygamepad::detect::PressedPad;
//! use anygamepad::model::raw::{RawLayout, RawSource, RawState};
//!
//! let layout = RawLayout {
//!     buttons: (0..4).map(RawSource::Index).collect(),
//!     ..RawLayout::default()
//! };
//! let mut picker = PressedPad::new();
//! let mut first = RawState::for_layout(&layout);
//! let second = RawState::for_layout(&layout);
//! assert!(!picker.update("first", &layout, &first));
//! assert!(!picker.update("second", &layout, &second));
//! first.buttons[2] = 1.0;
//! assert!(picker.update("first", &layout, &first));
//! ```

use std::collections::BTreeMap;

use anygamepad_model::raw::{Hat, RawLayout, RawState};

use crate::wizard::{MOVE, PRESS, RELEASED};

/// Picks the pad the person presses. See the [module documentation](self).
#[derive(Clone, Debug)]
pub struct PressedPad<K> {
    /// Where each pad's axes rest, once it was seen at rest.
    rest: BTreeMap<K, Vec<f32>>,
}

impl<K: Ord + Clone> Default for PressedPad<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone> PressedPad<K> {
    /// A picker that has seen no pad yet.
    pub fn new() -> Self {
        Self {
            rest: BTreeMap::new(),
        }
    }

    /// Feeds the state of pad `key`, read with `layout`; returns whether
    /// the pad is pressed after having been seen at rest. States that do
    /// not fit the layout are ignored.
    pub fn update(&mut self, key: K, layout: &RawLayout, raw: &RawState) -> bool {
        if raw.buttons.len() != layout.buttons.len()
            || raw.axes.len() != layout.axes.len()
            || raw.hats.len() != layout.hats.len()
        {
            return false;
        }
        let half = |index: usize| {
            let info = &layout.axes[index];
            ((info.max - info.min) / 2.0).abs().max(f32::EPSILON)
        };
        let quiet = raw
            .buttons
            .iter()
            .all(|&value| value.is_nan() || value < RELEASED)
            && raw.hats.iter().all(|&hat| hat == Hat::CENTERED);
        let Some(rest) = self.rest.get_mut(&key) else {
            if quiet {
                let rest = raw
                    .axes
                    .iter()
                    .enumerate()
                    .map(|(index, &value)| {
                        if value.is_finite() {
                            value
                        } else {
                            let info = &layout.axes[index];
                            (info.min + info.max) / 2.0
                        }
                    })
                    .collect();
                self.rest.insert(key, rest);
            }
            return false;
        };
        let moved =
            raw.axes
                .iter()
                .zip(rest.iter())
                .enumerate()
                .any(|(index, (&value, &at_rest))| {
                    value.is_finite() && ((value - at_rest) / half(index)).abs() >= MOVE
                });
        let pressed = raw.buttons.iter().any(|&value| value >= PRESS)
            || raw.hats.iter().any(|&hat| hat != Hat::CENTERED)
            || moved;
        if quiet && !moved {
            // Follow slow drift while nothing is used.
            for (at_rest, &value) in rest.iter_mut().zip(&raw.axes) {
                if value.is_finite() {
                    *at_rest = value;
                }
            }
        }
        pressed
    }

    /// Forgets pad `key`, which disconnected.
    pub fn forget(&mut self, key: &K) {
        self.rest.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use anygamepad_model::raw::{RawAxisInfo, RawSource};

    use super::*;

    fn layout() -> RawLayout {
        RawLayout {
            buttons: (0..4).map(RawSource::Index).collect(),
            axes: (0..2)
                .map(|index| RawAxisInfo {
                    source: RawSource::Index(index),
                    min: -1.0,
                    max: 1.0,
                })
                .collect(),
            hats: vec![RawSource::Index(0)],
            ..RawLayout::default()
        }
    }

    #[test]
    fn the_pressed_pad_is_picked() {
        let layout = layout();
        let mut picker = PressedPad::new();
        // Pad 1's second axis is a trigger resting at its low end.
        let mut one = RawState::for_layout(&layout);
        one.axes[1] = -1.0;
        let mut two = RawState::for_layout(&layout);
        assert!(!picker.update(1, &layout, &one));
        assert!(!picker.update(2, &layout, &two));
        // Noise and slow drift are not presses.
        one.axes[0] = 0.1;
        assert!(!picker.update(1, &layout, &one));
        one.axes[0] = 0.2;
        assert!(!picker.update(1, &layout, &one));
        two.hats[0] = Hat(1);
        assert!(picker.update(2, &layout, &two));
        one.axes[1] = 0.5;
        assert!(picker.update(1, &layout, &one));
    }

    #[test]
    fn a_pad_held_since_it_connected_waits_for_a_release() {
        let layout = layout();
        let mut picker = PressedPad::new();
        let mut held = RawState::for_layout(&layout);
        held.buttons[0] = 1.0;
        assert!(!picker.update("held", &layout, &held));
        assert!(!picker.update("held", &layout, &held));
        held.buttons[0] = 0.0;
        assert!(!picker.update("held", &layout, &held));
        held.buttons[3] = 1.0;
        assert!(picker.update("held", &layout, &held));
        picker.forget(&"held");
        assert!(!picker.update("held", &layout, &held));
    }

    #[test]
    fn misfit_states_and_nan_are_ignored() {
        let layout = layout();
        let mut picker = PressedPad::new();
        let mut state = RawState::for_layout(&layout);
        state.axes[0] = f32::NAN;
        assert!(!picker.update(0, &layout, &state));
        assert!(!picker.update(0, &layout, &state));
        let mut short = state.clone();
        short.buttons.pop();
        short.buttons[0] = 1.0;
        assert!(!picker.update(0, &layout, &short));
    }
}
