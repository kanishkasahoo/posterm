pub mod atomic_write;
pub mod collections;
pub mod config;
pub mod history;

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub use collections::{
    delete_collection_file, load_all_collections, save_collection, Collection, SavedRequest,
    SerializedKeyValueRow,
};
pub use config::{ensure_config_exists, save_config, AppConfig};
pub use history::{is_sensitive_header, load_history, save_history, HistoryEntry};

use crate::state::AppState;

/// Identifies which persistence target should be flushed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PersistTarget {
    #[allow(dead_code)]
    Config,
    /// Identified by the collection's UUID string.
    Collection(String),
    History,
}

/// Debounce interval: we wait this long after the last `schedule_save` before
/// actually writing to disk.
const DEBOUNCE_INTERVAL: Duration = Duration::from_millis(500);

/// Manages debounced, on-demand persistence of application state.
pub struct PersistenceManager {
    /// Path to the posterm configuration directory (exposed for inspection/tests).
    #[allow(dead_code)]
    pub config_dir: PathBuf,
    debounce_map: HashMap<PersistTarget, Instant>,
    debounce_interval: Duration,
}

impl PersistenceManager {
    pub fn new() -> Self {
        Self {
            config_dir: config::config_dir(),
            debounce_map: HashMap::new(),
            debounce_interval: DEBOUNCE_INTERVAL,
        }
    }

    /// Marks `target` as needing a save.  Repeated calls within the debounce
    /// window reset the timer.
    pub fn schedule_save(&mut self, target: PersistTarget) {
        self.debounce_map.insert(target, Instant::now());
    }

    /// Flushes all targets whose debounce interval has elapsed.
    ///
    /// Errors are printed to `stderr` and do not abort the application.
    pub fn flush_pending(&mut self, state: &AppState) {
        let now = Instant::now();
        let ready: Vec<PersistTarget> = self
            .debounce_map
            .iter()
            .filter(|(_, instant)| now.duration_since(**instant) >= self.debounce_interval)
            .map(|(target, _)| target.clone())
            .collect();

        for target in ready {
            self.debounce_map.remove(&target);
            let result = match &target {
                PersistTarget::Config => save_config(&state.config),
                PersistTarget::Collection(id) => {
                    if let Some(col) = state.collections.iter().find(|c| &c.id == id) {
                        save_collection(col)
                    } else {
                        // Collection was deleted before flush — nothing to do.
                        Ok(())
                    }
                }
                PersistTarget::History => save_history(
                    &state.history,
                    state.config.history_limit,
                    state.config.persist_sensitive_headers,
                ),
            };

            if let Err(e) = result {
                eprintln!("[posterm] persistence error for {target:?}: {e}");
            }
        }
    }
}

impl Default for PersistenceManager {
    fn default() -> Self {
        Self::new()
    }
}
