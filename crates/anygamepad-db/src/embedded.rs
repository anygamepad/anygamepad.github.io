//! The catalog and the inventory embedded at build time (H2): the build
//! script reads `data/controllers/` and `data/lab/inventory.toml`, checks
//! them and encodes them with postcard into `OUT_DIR`.

use std::sync::OnceLock;

use crate::catalog::{Catalog, decode_models};
use crate::lab::{Inventory, decode_inventory};

/// The catalog models of `data/controllers/`, in ID order.
const CATALOG_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/catalog.postcard"));

/// The inventory of `data/lab/inventory.toml`, or an empty one.
const INVENTORY_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/inventory.postcard"));

impl Catalog {
    /// The catalog embedded at build time from `data/controllers/`, in ID
    /// order.
    pub fn embedded() -> &'static Self {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| Catalog {
            models: decode_models(CATALOG_BYTES)
                .expect("the embedded catalog in OUT_DIR/catalog.postcard must be valid"),
        })
    }
}

impl Inventory {
    /// The inventory embedded at build time from `data/lab/inventory.toml`;
    /// empty when the repository has none.
    pub fn embedded() -> &'static Self {
        static INVENTORY: OnceLock<Inventory> = OnceLock::new();
        INVENTORY.get_or_init(|| {
            decode_inventory(INVENTORY_BYTES)
                .expect("the embedded inventory in OUT_DIR/inventory.postcard must be valid")
        })
    }
}
