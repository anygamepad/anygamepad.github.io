//! Lists the gamepads the Linux provider sees and prints their canonical
//! state while buttons change (C4). Rumbles a pad briefly when its south
//! button is pressed, if it can.
//!
//! ```sh
//! cargo run -p anygamepad --example linux_pads
//! ```
//!
//! Mappings come from the embedded database, else from the Linux gamepad
//! specification for drivers that follow it.

#[cfg(target_os = "linux")]
fn main() {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use anygamepad::db::Database;
    use anygamepad::mapping::Mapper;
    use anygamepad::model::raw::RawState;
    use anygamepad::model::{Button, GamepadState, OutputCommand, Rumble};
    use anygamepad::native::evdev::{follows_spec, spec_mapping};
    use anygamepad::native::linux::LinuxProvider;
    use anygamepad::provider::{Access, Provider, ProviderEvent};

    let database = Database::embedded();
    let mut provider = LinuxProvider::new();
    let mut pads = BTreeMap::new();
    let mut events = Vec::new();
    println!("Waiting for gamepads (Ctrl+C to quit)...");
    loop {
        events.clear();
        if let Err(error) = provider.update(&mut events) {
            eprintln!("update: {error}");
        }
        for event in &events {
            match *event {
                ProviderEvent::Added(id) => {
                    let info = provider.info(id).expect("added device").clone();
                    let identity = &info.identity;
                    let mapper = match Mapper::from_database(&database, identity, &info.layout) {
                        Some((mapper, profile)) => {
                            println!("  profile: {}", profile.name);
                            mapper
                        }
                        None if follows_spec(identity.driver.as_deref()) => {
                            println!("  no profile; Linux gamepad specification");
                            Mapper::new(&spec_mapping(&info.layout), &info.layout)
                        }
                        None => {
                            println!("  no profile, generic driver: raw values only");
                            Mapper::new(&Default::default(), &info.layout)
                        }
                    };
                    println!(
                        "+ {id:?} {} ({:04x}:{:04x}, {}, driver {}): {} buttons, {} axes, {} hats, sensors {:?}, touchpads {}, outputs {:?}",
                        identity.product_name.as_deref().unwrap_or("?"),
                        identity.vendor_id.unwrap_or(0),
                        identity.product_id.unwrap_or(0),
                        identity.transport,
                        identity.driver.as_deref().unwrap_or("?"),
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
                            pads.insert(id, (mapper, raw, GamepadState::new()));
                        }
                        Err(error) => eprintln!("  open: {error}"),
                    }
                }
                ProviderEvent::Removed(id) => {
                    println!("- {id:?}");
                    pads.remove(&id);
                }
            }
        }
        if provider.inaccessible() > 0 {
            eprintln!(
                "{} gamepad(s) without permission: see docs/linux-permissions.md",
                provider.inaccessible()
            );
        }
        for (&id, (mapper, raw, last)) in &mut pads {
            if provider.poll(id, raw).is_err() {
                continue;
            }
            let mut state = GamepadState::new();
            mapper.apply(raw, &mut state);
            if state.buttons != last.buttons {
                let pressed: Vec<_> = state.buttons.iter().map(|b| b.as_str()).collect();
                println!(
                    "{id:?} {pressed:?} left ({:.2}, {:.2}) right ({:.2}, {:.2}) triggers {:.2} {:.2}",
                    state.left_stick().x,
                    state.left_stick().y,
                    state.right_stick().x,
                    state.right_stick().y,
                    state.axis(anygamepad::model::Axis::LeftTrigger),
                    state.axis(anygamepad::model::Axis::RightTrigger),
                );
                let south = state.pressed(Button::South);
                if south != last.pressed(Button::South) {
                    let strength = if south { 0.5 } else { 0.0 };
                    let _ = provider.send(
                        id,
                        OutputCommand::Rumble(Rumble {
                            low_frequency: strength,
                            high_frequency: strength,
                        }),
                    );
                }
            }
            *last = state;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example needs Linux.");
}
