//! Dialogs that stay over the app that asked for them, like any app's own
//! dialog: when the app's window gets the focus while its dialog is open,
//! the dialog gets it back (IMP-355).
//!
//! Nova's dialogs (apps' Open and Save dialogs, through the portal) are
//! windows of their own app, with no parent window the compositor knows.
//! Over a fullscreen app that matters: niri and Hyprland draw a focused
//! fullscreen window over floating ones, so a click on the app, or the
//! pointer crossing onto it with focus-follows-mouse, hid the dialog behind
//! the app it belongs to, with nothing on screen to bring it back.
//!
//! The app a dialog belongs to is the window that had the focus when the
//! dialog first got it: a dialog opens because that window just asked for
//! it. Only that window is held back; any other one takes the focus as
//! usual. Which app ids are dialogs, the modules that open them say
//! ([`super::Compositor::keep_dialogs_up`]).

use nova_protocol::FocusedWindow;

/// What the focus moving does to the dialogs.
#[derive(Debug, Default)]
pub(super) struct Dialogs {
    /// Their app ids.
    app_ids: Vec<String>,
    /// The dialog that got the focus last, by the compositor's window id.
    dialog: Option<String>,
    /// The window it belongs to, by the compositor's window id.
    owner: Option<String>,
}

impl Dialogs {
    /// Windows with `app_id` are dialogs from now on.
    pub(super) fn add(&mut self, app_id: &str) {
        if !self.app_ids.iter().any(|known| known == app_id) {
            self.app_ids.push(app_id.to_owned());
        }
    }

    /// The focus moved from `before` to `now`. The dialogs' app ids when a
    /// dialog should take it back.
    pub(super) fn focused(&mut self, before: Option<&FocusedWindow>, now: Option<&FocusedWindow>) -> Option<&[String]> {
        let now = now?;
        if self.is_dialog(now) {
            // The window that had the focus as a new dialog opened asked for
            // it. Coming back to the same dialog keeps its app.
            if self.dialog.as_ref() != Some(&now.id) {
                self.dialog = Some(now.id.clone());
                self.owner = before.filter(|window| !self.is_dialog(window)).map(|window| window.id.clone());
            }
            return None;
        }
        (self.owner.as_ref() == Some(&now.id)).then_some(&self.app_ids[..])
    }

    /// No dialog is open any more: its app takes the focus as usual.
    pub(super) fn closed(&mut self) {
        self.dialog = None;
        self.owner = None;
    }

    fn is_dialog(&self, window: &FocusedWindow) -> bool {
        self.app_ids.iter().any(|app_id| app_id.eq_ignore_ascii_case(&window.app_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIALOG: &str = "nova-files-picker";

    fn window(id: u32, app_id: &str) -> FocusedWindow {
        FocusedWindow { id: id.to_string(), app_id: app_id.into(), title: String::new() }
    }

    fn dialogs() -> Dialogs {
        let mut dialogs = Dialogs::default();
        dialogs.add(DIALOG);
        dialogs.add(DIALOG);
        dialogs
    }

    #[test]
    fn the_app_that_asked_gives_the_focus_back_to_its_dialog() {
        let mut dialogs = dialogs();
        let (app, dialog) = (window(1, "zen"), window(2, DIALOG));
        assert_eq!(dialogs.focused(Some(&app), Some(&dialog)), None);
        assert_eq!(dialogs.focused(Some(&dialog), Some(&app)), Some(&[DIALOG.to_owned()][..]));
        // After the dialog took it back, again.
        assert_eq!(dialogs.focused(Some(&app), Some(&dialog)), None);
        assert!(dialogs.focused(Some(&dialog), Some(&app)).is_some(), "still its app");
    }

    #[test]
    fn other_windows_take_the_focus_as_usual() {
        let mut dialogs = dialogs();
        let (app, dialog, other) = (window(1, "zen"), window(2, DIALOG), window(3, "ghostty"));
        dialogs.focused(Some(&app), Some(&dialog));
        assert_eq!(dialogs.focused(Some(&dialog), Some(&other)), None);
        // Going back to the dialog from elsewhere doesn't make that its app.
        dialogs.focused(Some(&other), Some(&dialog));
        assert_eq!(dialogs.focused(Some(&dialog), Some(&other)), None);
        assert!(dialogs.focused(Some(&other), Some(&app)).is_some(), "its app still waits for it");
    }

    #[test]
    fn a_closed_dialog_holds_nothing_back() {
        let mut dialogs = dialogs();
        let (app, dialog) = (window(1, "zen"), window(2, DIALOG));
        dialogs.focused(Some(&app), Some(&dialog));
        dialogs.closed();
        assert_eq!(dialogs.focused(Some(&dialog), Some(&app)), None);
    }

    #[test]
    fn a_dialog_opened_without_a_focused_window_has_no_app() {
        let mut dialogs = dialogs();
        let (app, dialog) = (window(1, "zen"), window(2, DIALOG));
        dialogs.focused(None, Some(&dialog));
        assert_eq!(dialogs.focused(Some(&dialog), Some(&app)), None);
        assert_eq!(dialogs.focused(Some(&app), None), None, "no window has the focus");
    }

    #[test]
    fn without_dialogs_nothing_changes() {
        let mut dialogs = Dialogs::default();
        let (app, dialog) = (window(1, "zen"), window(2, DIALOG));
        dialogs.focused(Some(&app), Some(&dialog));
        assert_eq!(dialogs.focused(Some(&dialog), Some(&app)), None);
    }
}
