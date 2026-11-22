//! The Nova Files feature's messages. It has no commands or events of its
//! own: it answers the launcher's Files mode, so all it names on the wire
//! is the launcher provider view it answers for. The work is
//! `nova-nova-files` (`features/nova-files/core`).

/// The launcher provider that finds files (feature `nova-files`): the
/// launcher's Files mode, which searches the names the Nova Files app
/// indexes and opens or shows what it finds there.
pub const PROVIDER_FILES: &str = "files";
