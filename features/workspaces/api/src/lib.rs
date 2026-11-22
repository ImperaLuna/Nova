//! The workspaces feature's messages on novad's socket. The workspaces it
//! shows come in the compositor's own event (`nova_desktop_info::Event`);
//! these are what a click or a scroll on them asks for.

use nova_desktop_info::{Step, WorkspaceId};
use serde::{Deserialize, Serialize};

/// Sent by the UI. Arguments with unknown fields are refused.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    /// Shows a workspace (capability `focus_workspace`), focusing its output.
    FocusWorkspace { id: WorkspaceId },
    /// Shows the workspace after or before the active one on `output`
    /// (capability `focus_workspace`), like scrolling on the bar. Nothing
    /// happens at either end.
    StepWorkspace { output: String, step: Step },
}
