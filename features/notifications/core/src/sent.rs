//! What apps send, checked and cut down: text made plain, pictures checked
//! and written out, names checked, lists capped.

use super::*;

impl Shared {
    /// What `sent` becomes as notification `id`, its text and pictures checked.
    pub(crate) fn read(&self, id: u32, mut sent: Sent, now: i64) -> Notification {
        let desktop_entry = take_str(&mut sent.hints, "desktop-entry")
            .map(|entry| entry.strip_suffix(".desktop").unwrap_or(&entry).to_string())
            .filter(|entry| is_name(entry));
        // Like GNOME: the installed app the sender names is shown as its
        // desktop entry says (the user may have changed its name or icon
        // there), and what the sender calls itself only when none is. Right
        // after novad starts, it waits for the apps to be read, or a muted
        // app would get past its mute under the sender's name.
        let installed =
            desktop_entry.as_deref().and_then(|entry| self.apps.app_once_scanned(&format!("{entry}.desktop")));
        let (installed_name, installed_icon) = installed.map_or((None, None), |app| (Some(app.name), app.icon));
        let app = [installed_name.as_deref(), Some(sent.app_name.as_str()), desktop_entry.as_deref()]
            .into_iter()
            .flatten()
            .map(|name| text::cut(name.trim(), MAX_LABEL_CHARS))
            .find(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown app".into());
        let icons = &self.sources.icons;
        let icon = installed_icon
            .or_else(|| icon(&sent.app_icon, icons))
            .or_else(|| desktop_entry.as_deref().and_then(|entry| icons.find(entry)));
        let urgency = match take(&mut sent.hints, "urgency").as_deref() {
            Some(Variant::U8(0)) => Urgency::Low,
            Some(Variant::U8(2)) => Urgency::Critical,
            _ => Urgency::Normal,
        };
        let progress = match take(&mut sent.hints, "value").as_deref() {
            Some(Variant::I32(value)) => Some((*value).clamp(0, 100) as u8),
            Some(Variant::U32(value)) => Some((*value).min(100) as u8),
            _ => None,
        };
        let image = self.image(id, &mut sent.hints);
        let mut actions = Vec::new();
        let mut has_default = false;
        for [key, label] in sent.actions.as_chunks::<2>().0 {
            let label = text::cut(label.trim(), MAX_LABEL_CHARS);
            if key == "default" {
                has_default = true;
            } else if !key.is_empty()
                && key.len() <= MAX_NAME_LEN
                && !label.is_empty()
                && actions.len() < MAX_ACTIONS
                && !actions.iter().any(|known: &NotificationAction| &known.key == key)
            {
                actions.push(NotificationAction { key: key.clone(), label });
            }
        }
        // Critical ones stay until they're closed, whatever the app asked.
        let timeout_ms = match (urgency, sent.expire_timeout) {
            (Urgency::Critical, _) => Some(0),
            (_, timeout) => u32::try_from(timeout).ok(),
        };
        Notification {
            id,
            app,
            icon,
            app_id: desktop_entry.map(|entry| format!("{entry}.desktop")),
            summary: text::cut(sent.summary.replace(['\n', '\r'], " ").trim(), MAX_SUMMARY_CHARS),
            body: text::plain(&sent.body, MAX_BODY_CHARS),
            image,
            actions,
            has_default,
            urgency,
            progress,
            timeout_ms,
            time: now,
            unread: false,
            live: true,
            device: None,
        }
    }

    /// The picture in `hints`: pixels, written out, or a file.
    fn image(&self, id: u32, hints: &mut HashMap<String, OwnedVariant>) -> Option<String> {
        // Older senders use the spec's earlier names.
        for key in ["image-data", "image_data", "icon_data"] {
            let Some(raw) = hints.remove(key) else { continue };
            let saved = image::Raw::try_from(raw).map_err(|e| format!("not a picture: {e}")).and_then(|raw| {
                image::save(raw, &self.sources.images, id, self.revision.fetch_add(1, Ordering::Relaxed))
            });
            match saved {
                Ok(path) => return Some(path.display().to_string()),
                Err(e) => self.logger.log(LogLevel::Debug, LOG_SOURCE, &format!("skipped {key}: {e}")),
            }
        }
        ["image-path", "image_path"]
            .into_iter()
            .find_map(|key| take_str(hints, key))
            .and_then(|reference| nova_paths::reference::file(&reference))
            .map(|path| path.display().to_string())
    }
}

/// The file for an app icon as `Notify` gives it: a file, or a theme icon
/// name looked up in `icons`.
pub(crate) fn icon(reference: &str, icons: &Icons) -> Option<String> {
    let reference = reference.trim();
    if let Some(path) = nova_paths::reference::file(reference) {
        return Some(path.display().to_string());
    }
    is_name(reference).then(|| icons.find(reference)).flatten()
}

/// A desktop entry id or icon name: short, and nothing that reads as a path.
fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
        && !name.starts_with('.')
}

fn take(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> Option<OwnedVariant> {
    hints.remove(key)
}

pub(crate) fn take_bool(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> bool {
    matches!(take(hints, key).as_deref(), Some(Variant::Bool(true)))
}

fn take_str(hints: &mut HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    match take(hints, key).as_deref() {
        Some(Variant::Str(text)) => Some(text.as_str().to_string()),
        _ => None,
    }
}
