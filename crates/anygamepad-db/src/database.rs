//! In-memory device database holding compile-time embedded profiles and
//! runtime overrides (ADR 0003, task D4).

use std::sync::{Arc, OnceLock};

use anygamepad_model::identity::DeviceIdentity;

#[cfg(feature = "toml")]
use crate::ProfileError;
use crate::wire;
use crate::{Profile, best_match};

/// Curated profiles embedded at compile time from `data/profiles/*.toml`.
const CURATED_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/curated.postcard"));

/// SDL GameControllerDB profiles embedded at compile time from
/// `data/sdl-gamecontrollerdb/gamecontrollerdb.txt`.
#[cfg(all(feature = "embedded-sdl", not(target_arch = "wasm32")))]
const SDL_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/sdl.postcard"));

fn embedded_curated_arc() -> Arc<[Profile]> {
    static PROFILES: OnceLock<Arc<[Profile]>> = OnceLock::new();
    PROFILES
        .get_or_init(|| {
            let list = wire::decode_profiles(CURATED_BYTES)
                .expect("embedded curated profiles in OUT_DIR/curated.postcard must be valid");
            Arc::from(list)
        })
        .clone()
}

#[cfg(all(feature = "embedded-sdl", not(target_arch = "wasm32")))]
fn embedded_all_arc() -> Arc<[Profile]> {
    static PROFILES: OnceLock<Arc<[Profile]>> = OnceLock::new();
    PROFILES
        .get_or_init(|| {
            let mut list = wire::decode_profiles(CURATED_BYTES)
                .expect("embedded curated profiles in OUT_DIR/curated.postcard must be valid");
            let sdl = wire::decode_profiles(SDL_BYTES)
                .expect("embedded SDL profiles in OUT_DIR/sdl.postcard must be valid");
            list.extend(sdl);
            Arc::from(list)
        })
        .clone()
}

/// In-memory device database holding embedded profiles and runtime overrides.
///
/// Lookups via [`best_match`](Self::best_match) check runtime overrides first,
/// then embedded profiles. On ties, runtime overrides win.
#[derive(Clone, Debug, PartialEq)]
pub struct Database {
    overrides: Vec<Profile>,
    embedded: Arc<[Profile]>,
}

impl Database {
    /// An empty database with no profiles.
    pub fn empty() -> Self {
        Self {
            overrides: Vec::new(),
            embedded: Arc::from([]),
        }
    }

    /// Loads the compile-time embedded database.
    ///
    /// On `wasm32` targets, embeds all curated profiles from `data/profiles/`.
    /// On native targets with `embedded-sdl` enabled, also embeds all converted
    /// entries from SDL GameControllerDB.
    pub fn embedded() -> Self {
        #[cfg(all(feature = "embedded-sdl", not(target_arch = "wasm32")))]
        {
            Self {
                overrides: Vec::new(),
                embedded: embedded_all_arc(),
            }
        }
        #[cfg(not(all(feature = "embedded-sdl", not(target_arch = "wasm32"))))]
        {
            Self {
                overrides: Vec::new(),
                embedded: embedded_curated_arc(),
            }
        }
    }

    /// Loads only the compile-time curated profiles from `data/profiles/`.
    pub fn embedded_curated() -> Self {
        Self {
            overrides: Vec::new(),
            embedded: embedded_curated_arc(),
        }
    }

    /// Creates a database with the given profiles as its base.
    pub fn with_profiles(profiles: impl IntoIterator<Item = Profile>) -> Self {
        Self {
            overrides: Vec::new(),
            embedded: Arc::from(profiles.into_iter().collect::<Vec<_>>()),
        }
    }

    /// Adds a runtime override profile.
    ///
    /// Overrides take precedence over embedded profiles in
    /// [`best_match`](Self::best_match) on ties.
    pub fn add_override(&mut self, profile: Profile) {
        self.overrides.push(profile);
    }

    /// Parses and validates a TOML profile string, and adds it as a runtime
    /// override.
    #[cfg(feature = "toml")]
    pub fn load_override_toml(&mut self, text: &str) -> Result<&Profile, ProfileError> {
        let profile = Profile::from_toml(text)?;
        self.overrides.push(profile);
        Ok(self.overrides.last().unwrap())
    }

    /// All profiles in lookup order: runtime overrides first, then embedded
    /// profiles.
    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.overrides.iter().chain(self.embedded.iter())
    }

    /// Picks the profile that matches `identity` best.
    ///
    /// Runtime overrides are checked with precedence on ties over embedded
    /// profiles.
    pub fn best_match(&self, identity: &DeviceIdentity) -> Option<&Profile> {
        best_match(self.iter(), identity)
    }

    /// Total number of profiles in this database.
    pub fn len(&self) -> usize {
        self.overrides.len() + self.embedded.len()
    }

    /// Whether this database contains no profiles.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Runtime overrides added to this database.
    pub fn overrides(&self) -> &[Profile] {
        &self.overrides
    }

    /// Embedded profiles in this database.
    pub fn embedded_profiles(&self) -> &[Profile] {
        &self.embedded
    }
}

impl Default for Database {
    fn default() -> Self {
        Self::embedded()
    }
}
