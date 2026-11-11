//! The macOS native providers (C6).
//!
//! - [`IoHidProvider`]: HID game controllers through IOKit's
//!   `IOHIDManager`: generic pads through their report descriptor, and the
//!   PlayStation and Switch Pro pads through their protocol drivers when
//!   asked ([`IoHidProvider::set_game_controller_pads`]).
//! - [`GameControllerProvider`]: the pads macOS's GameController framework
//!   drives (PlayStation, Xbox, Switch Pro, MFi), which it has already
//!   normalized.
//!
//! By default each pad belongs to one of them: GameController drives the
//! PlayStation, Xbox and Switch Pro pads and switches the PlayStation pads
//! to a report IOKit readers of the standard report cannot use, so the
//! IOKit provider leaves those to it (see [`iokit`](super::iokit)).
//!
//! # Threads and run loops
//!
//! The IOKit provider schedules its sources on the run loop of the thread
//! that creates it, in a private mode it runs itself in `update` and
//! `poll`: create it on the thread that drives it. GameController delivers
//! connections and input through the main thread's run loop, which an app
//! with a window runs; a command-line program runs it itself (see the
//! `macos_pads` example).
//!
//! # Unsafe code
//!
//! IOKit and GameController are C and Objective-C APIs: every call into
//! them is `unsafe` in their bindings (`objc2-io-kit`,
//! `objc2-game-controller`), and each block says why it is sound.
#![allow(unsafe_code)]

mod gc;
mod iohid;

pub use gc::GameControllerProvider;
pub use iohid::{IoHidProvider, RESCAN_INTERVAL};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{Access, Provider, ProviderEvent};

    /// Checks every device a provider lists right after it starts: these
    /// tests run on CI's macOS runners, usually with no game controller,
    /// where they still call into IOKit and GameController.
    fn list(provider: &mut dyn Provider) {
        let mut events = Vec::new();
        provider.update(&mut events).unwrap();
        provider.update(&mut events).unwrap();
        for event in events {
            let ProviderEvent::Added(id) = event else {
                continue;
            };
            let info = provider.info(id).unwrap().clone();
            assert_eq!(info.identity.os, anygamepad_model::identity::Os::MacOs);
            if provider.open(id, Access::ObserveOnly).is_ok() {
                let mut state = anygamepad_model::raw::RawState::for_layout(&info.layout);
                let _ = provider.poll(id, &mut state);
                provider.close(id);
            }
        }
    }

    #[test]
    fn iokit_provider_starts() {
        let mut provider = IoHidProvider::new().unwrap();
        list(&mut provider);
        provider.set_game_controller_pads(true);
        list(&mut provider);
    }

    #[test]
    fn game_controller_provider_starts() {
        let mut provider = GameControllerProvider::new().unwrap();
        list(&mut provider);
        assert_eq!(
            provider.open(crate::provider::DeviceId(u64::MAX), Access::Shared),
            Err(crate::provider::ProviderError::UnknownDevice)
        );
    }
}
