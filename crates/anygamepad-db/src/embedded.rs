//! The catalog, the inventory (H2) and the test plans (H3) embedded at
//! build time: the build script reads `data/controllers/`,
//! `data/lab/inventory.toml` and `data/test-plans/`, checks them and
//! encodes them with postcard into `OUT_DIR`.

use std::sync::OnceLock;

use crate::catalog::{Catalog, decode_models};
use crate::lab::{Inventory, decode_inventory};
use crate::plan::{TestPlan, decode_plans};

/// The catalog models of `data/controllers/`, in ID order.
const CATALOG_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/catalog.postcard"));

/// The inventory of `data/lab/inventory.toml`, or an empty one.
const INVENTORY_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/inventory.postcard"));

/// The test plans of `data/test-plans/`, in ID order.
const PLAN_BYTES: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/plans.postcard"));

impl TestPlan {
    /// The test plans embedded at build time from `data/test-plans/`, in
    /// ID order.
    pub fn embedded() -> &'static [TestPlan] {
        static PLANS: OnceLock<Vec<TestPlan>> = OnceLock::new();
        PLANS.get_or_init(|| {
            decode_plans(PLAN_BYTES)
                .expect("the embedded plans in OUT_DIR/plans.postcard must be valid")
        })
    }

    /// The embedded plan with this ID.
    pub fn get(id: &str) -> Option<&'static TestPlan> {
        Self::embedded().iter().find(|plan| plan.id == id)
    }
}

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
