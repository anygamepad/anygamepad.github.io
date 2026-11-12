//! Lists the gamepads the macOS providers see and prints their canonical
//! state while buttons change (C6). Moves a pad's lights to the next
//! player and colour when east is pressed, if it can.
//!
//! ```sh
//! # GameController pads (PlayStation, Xbox, Switch Pro, MFi) and the
//! # other HID pads through IOKit:
//! cargo run -p anygamepad --example macos_pads
//! # Every HID pad through IOKit, with the protocol drivers (PlayStation,
//! # Switch Pro), GameController left out:
//! cargo run -p anygamepad --example macos_pads -- --iokit
//! ```
//!
//! GameController delivers connections and input through the main run
//! loop, which this program runs between polls. Generic IOKit pads get
//! their mapping from the embedded database; without one, only their raw
//! values change.

#[cfg(target_os = "macos")]
fn main() {
    use std::collections::BTreeMap;

    use anygamepad::db::Database;
    use anygamepad::mapping::Mapper;
    use anygamepad::model::raw::RawState;
    use anygamepad::model::{Button, GamepadState, OutputCommand, Rgb};
    use anygamepad::native::macos::{GameControllerProvider, IoHidProvider};
    use anygamepad::provider::{Access, Provider, ProviderEvent};
    use objc2_core_foundation::{CFRunLoop, kCFRunLoopDefaultMode};

    const COLOURS: [Rgb; 4] = [
        Rgb { r: 0, g: 0, b: 255 },
        Rgb { r: 255, g: 0, b: 0 },
        Rgb { r: 0, g: 255, b: 0 },
        Rgb {
            r: 255,
            g: 0,
            b: 255,
        },
    ];

    let database = Database::embedded();
    let iokit_only = std::env::args().any(|arg| arg == "--iokit");
    let mut providers: Vec<(&str, Box<dyn Provider>)> = Vec::new();
    match IoHidProvider::new() {
        Ok(mut provider) => {
            provider.set_game_controller_pads(iokit_only);
            providers.push(("iokit", Box::new(provider)));
        }
        Err(error) => eprintln!("IOKit: {error}"),
    }
    if !iokit_only {
        match GameControllerProvider::new() {
            Ok(provider) => providers.push(("gc", Box::new(provider))),
            Err(error) => eprintln!("GameController: {error}"),
        }
    }

    let mut player = 0u8;
    let mut pads = BTreeMap::new();
    let mut events = Vec::new();
    println!("Waiting for gamepads (Ctrl+C to quit)...");
    loop {
        // GameController's connections and input arrive on the main run
        // loop; run it for up to 8 ms.
        // SAFETY: the default mode is a constant of CoreFoundation.
        let mode = unsafe { kCFRunLoopDefaultMode };
        CFRunLoop::run_in_mode(mode, 0.008, false);

        for (index, (name, provider)) in providers.iter_mut().enumerate() {
            events.clear();
            if let Err(error) = provider.update(&mut events) {
                eprintln!("{name} update: {error}");
            }
            for event in &events {
                match *event {
                    ProviderEvent::Added(id) => {
                        let info = provider.info(id).expect("added device").clone();
                        let identity = &info.identity;
                        let mapper = match Mapper::from_database(&database, identity, &info.layout)
                        {
                            Some((mapper, profile)) => {
                                println!("  profile: {}", profile.name);
                                mapper
                            }
                            None => Mapper::new(&Default::default(), &info.layout),
                        };
                        println!(
                            "+ {name} {id:?} {} ({:04x}:{:04x}, {}, {}): {} buttons, {} axes, {} hats, sensors {:?}, touchpads {}, outputs {:?}",
                            identity.product_name.as_deref().unwrap_or("?"),
                            identity.vendor_id.unwrap_or(0),
                            identity.product_id.unwrap_or(0),
                            identity.transport,
                            identity.driver.as_deref().unwrap_or("-"),
                            info.layout.buttons.len(),
                            info.layout.axes.len(),
                            info.layout.hats.len(),
                            info.layout.sensors,
                            info.layout.touchpads.len(),
                            info.outputs,
                        );
                        match provider.open(id, Access::Shared) {
                            Ok(()) => {
                                let raw = RawState::for_layout(&info.layout);
                                pads.insert((index, id), (mapper, raw, GamepadState::new()));
                            }
                            Err(error) => eprintln!("  open: {error}"),
                        }
                    }
                    ProviderEvent::Removed(id) => {
                        println!("- {name} {id:?}");
                        pads.remove(&(index, id));
                    }
                }
            }
            if provider.inaccessible() > 0 {
                eprintln!(
                    "{name}: {} gamepad(s) macOS refused to open (Input Monitoring in System Settings, Privacy & Security?)",
                    provider.inaccessible()
                );
            }
        }

        for (&(index, id), (mapper, raw, last)) in &mut pads {
            let (name, provider) = &mut providers[index];
            if provider.poll(id, raw).is_err() {
                continue;
            }
            let mut state = GamepadState::new();
            mapper.apply(raw, &mut state);
            if state.buttons != last.buttons {
                let pressed: Vec<_> = state.buttons.iter().map(|b| b.as_str()).collect();
                println!(
                    "{name} {id:?} {pressed:?} left ({:.2}, {:.2}) right ({:.2}, {:.2}) triggers {:.2} {:.2}",
                    state.left_stick().x,
                    state.left_stick().y,
                    state.right_stick().x,
                    state.right_stick().y,
                    state.axis(anygamepad::model::Axis::LeftTrigger),
                    state.axis(anygamepad::model::Axis::RightTrigger),
                );
                if state.pressed(Button::East) && !last.pressed(Button::East) {
                    player = player % 4 + 1;
                    let colour = COLOURS[usize::from(player) - 1];
                    let _ = provider.send(id, OutputCommand::PlayerNumber(player));
                    let _ = provider.send(id, OutputCommand::Lightbar(colour));
                }
            }
            if let Some(sample) = raw.motion.last() {
                if state.buttons.contains(Button::South) {
                    println!("{name} {id:?} motion {sample:?}");
                }
            }
            *last = state;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This example needs macOS.");
}
