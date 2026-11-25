mod list;
mod numbers;

use gpui::{App, AppContext};
use nova_look::tokens::WORKSPACE_INSET;
use nova_stage::Part;

const LIST_PART: &str = "workspaces.list";

pub fn register(cx: &mut App) {
    nova_stage::part(LIST_PART, cx, |screen, cx| {
        let screen = screen.clone();
        Part::new(cx.new(|cx| list::WorkspaceList::new(screen, cx))).inset(WORKSPACE_INSET)
    });
}
