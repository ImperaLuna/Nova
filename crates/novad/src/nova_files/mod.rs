//! The standalone Nova Files app (feature `nova-files`): its search over
//! D-Bus answers the launcher's Files mode instead of the `files` feature's
//! own index, which then isn't built for the launcher at all.
//!
//! Nova Files serves `io.github.ImperaLuna.NovaFiles.Search1` at its app id,
//! which the bus starts it for (D-Bus activation) and which it leaves after
//! a few idle minutes. `Search` returns at once and its stages come as
//! `Found` signals sent only to the caller, until one is `done`; a search
//! waits [`STAGES_WAIT`] for that, then answers with the stages it has.
//! Starting a search cancels the one before if it's still going.
//!
//! One connection, kept while the feature runs, with one thread reading the
//! signals and handing each stage to the search it belongs to.
//!
//! Only name searches come here (`files::launcher`, linked in
//! [`crate::files::Files`]). The `files` feature still answers the typed
//! paths (`~/…`, `/…`) and runs every item, opening files in their apps and
//! folders in Nova's own file manager, until the Nova Files app is ready to
//! take those over too.

mod client;

use crate::session_bus::Bus;
use client::Client;
use nova_protocol::FeatureState;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

pub const ID: &str = "nova-files";
const LOG_SOURCE: &str = "nova-files";
/// How long a search waits for its last stage. Nova Files answers a warm
/// search of a real Home in about 30 ms; past this, what came is shown.
pub const STAGES_WAIT: Duration = Duration::from_millis(100);
/// How long a call (and connecting) may take. The first call starts Nova
/// Files, so it gets longer than a search waits for stages.
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

/// The connection to Nova Files, while the feature runs.
pub struct NovaFiles {
    client: Result<Client, String>,
}

impl NovaFiles {
    /// The paths Nova Files finds for `query`, best first, at most `limit`
    /// per stage.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<PathBuf>, String> {
        self.client()?.search(query, limit, STAGES_WAIT)
    }

    fn client(&self) -> Result<&Client, String> {
        self.client.as_ref().map_err(|e| format!("Nova Files isn't reachable: {e}"))
    }
}

impl crate::modules::Module for NovaFiles {
    const ID: &'static str = ID;
    type Sources = Bus;
    type Config = ();

    fn sources(_: &crate::modules::Env) -> Bus {
        Bus::Session
    }

    fn config(_: &crate::config::Config) {}

    fn start(ctx: &crate::modules::Context, bus: &Bus, _: &(), _: &[FeatureState]) -> Arc<Self> {
        let client = Client::connect(bus, CALL_TIMEOUT);
        if let Err(e) = &client {
            ctx.logger.warn(LOG_SOURCE, &format!("can't reach Nova Files, so the launcher finds no files: {e}"));
        }
        Arc::new(Self { client })
    }

    fn stop(&self) {
        if let Ok(client) = &self.client {
            client.stop();
        }
    }
}

#[cfg(test)]
pub mod stand_in;
#[cfg(test)]
mod tests;
