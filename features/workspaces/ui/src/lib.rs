//! The workspaces' ui: the screen's workspaces in the central pill, as the
//! `workspaces.list` part (`features/workspaces/feature.toml`). The bar's
//! central pill draws the pill around it, the dividers beside it, and
//! steps through the workspaces on a scroll over the whole pill; this
//! draws the numbers and goes to one on a click.
//!
//! What it shows comes from novad's compositor event, the same on every
//! compositor: the workspaces of the screen's output that have windows or show
//! ([`numbers::shown`]). The one shown sits in a capsule that slides between
//! numbers, the accent while the output has the focus and a quiet fill
//! otherwise; a workspace whose window wants attention gets an accent number
//! and a dot. Special workspaces (Hyprland's scratchpad) lead, a little apart.

mod list;
mod numbers;

use gpui::{App, AppContext};
use nova_look::tokens::WORKSPACE_INSET;
use nova_stage::Part;

/// The view id the manifest's slot names.
const LIST_PART: &str = "workspaces.list";

pub fn register(cx: &mut App) {
    nova_stage::part(LIST_PART, cx, |screen, cx| {
        let screen = screen.clone();
        // Round cells sit as far from the pill's ends as from its top.
        Part::new(cx.new(|cx| list::WorkspaceList::new(screen, cx))).inset(WORKSPACE_INSET)
    });
}
