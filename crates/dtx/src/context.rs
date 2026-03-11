//! Application context for CLI commands.

use anyhow::Result;
use dtx_core::store::ConfigStore;

/// Application context providing access to config store.
pub struct Context {
    pub store: ConfigStore,
}

impl Context {
    /// Creates a new context by discovering and loading the project config.
    pub fn new() -> Result<Self> {
        let store = ConfigStore::discover_and_load().map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(Self { store })
    }
}
