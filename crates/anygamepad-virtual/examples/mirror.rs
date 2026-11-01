//! Mirrors the first physical gamepad into a virtual one of another type,
//! like DS4Windows (A5): the physical pad is grabbed so games only see the
//! virtual one, and the rumble games send goes back to the physical pad.
//!
//! ```sh
//! cargo run -p anygamepad-virtual --example mirror -- dualshock4
//! ```
//!
//! Targets: `xbox360`, `dualshock4`, `dualsense`, `switch_pro` (the Switch
//! Pro Controller does not answer `hid-nintendo`'s set-up yet). Needs
//! `/dev/uinput` or `/dev/uhid` access (docs/linux-permissions.md).

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::time::Duration;

    use anygamepad::db::Database;
    use anygamepad::mapping::Mapper;
    use anygamepad::model::raw::RawState;
    use anygamepad::model::{GamepadState, OutputCommand};
    use anygamepad::native::evdev::spec_mapping;
    use anygamepad::native::linux::LinuxProvider;
    use anygamepad::provider::{Access, Provider, ProviderEvent};
    use anygamepad_virtual::Target;
    use anygamepad_virtual::linux::LinuxVirtualPad;

    let target: Target = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "xbox360".into())
        .parse()?;
    let database = Database::embedded();
    let mut provider = LinuxProvider::new();
    let mut events = Vec::new();
    println!("Waiting for a gamepad...");
    let (id, mapper, mut raw) = loop {
        events.clear();
        provider.update(&mut events)?;
        let found = events.iter().find_map(|event| match *event {
            ProviderEvent::Added(id) => {
                let info = provider.info(id)?;
                // Skip virtual pads, ours included.
                let container = info.location.container().unwrap_or("");
                (!container.contains("/devices/virtual/")).then_some(id)
            }
            ProviderEvent::Removed(_) => None,
        });
        if let Some(id) = found {
            let info = provider.info(id).expect("added").clone();
            let mapper = Mapper::from_database(&database, &info.identity, &info.layout)
                .map(|(mapper, _)| mapper)
                .unwrap_or_else(|| {
                    let mapping = spec_mapping(&info.layout, info.identity.driver.as_deref());
                    Mapper::new(&mapping, &info.layout)
                });
            println!(
                "Mirroring {} as {target}",
                info.identity.product_name.as_deref().unwrap_or("a gamepad")
            );
            break (id, mapper, RawState::for_layout(&info.layout));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    provider.open(id, Access::Exclusive)?;
    let mut virtual_pad = LinuxVirtualPad::create(target, 0)?;
    let mut state = GamepadState::new();
    loop {
        events.clear();
        provider.update(&mut events)?;
        if events.contains(&ProviderEvent::Removed(id)) {
            println!("The gamepad was removed.");
            return Ok(());
        }
        provider.poll(id, &mut raw)?;
        mapper.apply(&raw, &mut state);
        for command in virtual_pad.update(&state)? {
            if let OutputCommand::Rumble(_) = command {
                let _ = provider.send(id, command);
            }
        }
        std::thread::sleep(Duration::from_millis(4));
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example needs Linux.");
}
