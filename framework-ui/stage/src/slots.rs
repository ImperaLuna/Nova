use gpui::{App, AppContext, Context, Entity, SharedString};
use nova_daemon_info::{Event, FeatureState, FeatureStatus, Slot, topics};
use nova_ui_client::Live;

pub const HOST_BAR_LEFT: &str = "bar.left";
pub const HOST_BAR_CENTER: &str = "bar.center";
pub const HOST_BAR_RIGHT: &str = "bar.right";
pub const HOST_STAGE_PANELS: &str = "stage.panels";
pub const HOST_SHELL_OVERLAYS: &str = "shell.overlays";

pub fn for_host(features: &[FeatureState], host: &str) -> Vec<SharedString> {
    let mut found: Vec<(i64, &str, &str)> = features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Ready)
        .flat_map(|feature| feature.slots.iter().map(move |slot| (feature, slot)))
        .filter(|(_, slot)| slot.host == host)
        .map(|(feature, slot)| (slot.order, feature.id.as_str(), slot.view.as_str()))
        .collect();
    found.sort();
    let mut views: Vec<SharedString> = Vec::new();
    for (_, _, view) in found {
        if !views.iter().any(|seen| seen.as_ref() == view) {
            views.push(SharedString::from(view.to_owned()));
        }
    }
    views
}

pub struct Slots {
    features: Entity<Live<Vec<FeatureState>>>,
    extra: FeatureState,
}

impl Slots {
    pub(crate) fn follow(extra: Vec<Slot>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx: &mut Context<Self>| {
            let features = Live::follow(topics::FEATURES, cx, |event| match event {
                Event::Features { features } => Some(features),
                _ => None,
            });
            cx.observe(&features, |_, _, cx| cx.notify()).detach();
            let extra = FeatureState {
                id: String::new(),
                status: FeatureStatus::Ready,
                missing: Vec::new(),
                slots: extra,
                mode: None,
            };
            Self { features, extra }
        })
    }

    pub fn for_host(&self, host: &str, cx: &App) -> Vec<SharedString> {
        let mut features = self.features.read(cx).get().cloned().unwrap_or_default();
        features.push(self.extra.clone());
        for_host(&features, host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(id: &str, status: FeatureStatus, slots: &[(&str, &str, i64)]) -> FeatureState {
        let slots =
            slots.iter().map(|(host, view, order)| Slot { host: (*host).into(), view: (*view).into(), order: *order });
        FeatureState { id: id.into(), status, missing: Vec::new(), slots: slots.collect(), mode: None }
    }

    #[test]
    fn a_host_lists_its_ready_features_by_order_then_id() {
        let features = [
            feature("media", FeatureStatus::Ready, &[(HOST_BAR_LEFT, "media.pill", 20)]),
            feature("tray", FeatureStatus::Off, &[(HOST_BAR_LEFT, "tray.pill", 5)]),
            feature("recording", FeatureStatus::MissingDeps, &[(HOST_BAR_LEFT, "recording.pill", 5)]),
            feature("ai-usage", FeatureStatus::Ready, &[(HOST_BAR_LEFT, "ai-usage.pill", 10)]),
            feature("bar", FeatureStatus::Ready, &[(HOST_BAR_CENTER, "central-pill", 10), (HOST_BAR_LEFT, "z", 10)]),
        ];
        assert_eq!(for_host(&features, HOST_BAR_LEFT), ["ai-usage.pill", "z", "media.pill"]);
        assert_eq!(for_host(&features, HOST_BAR_CENTER), ["central-pill"]);
        assert!(for_host(&features, HOST_STAGE_PANELS).is_empty());
    }

    #[test]
    fn a_view_two_features_list_shows_once() {
        let features = [
            feature("screenshot", FeatureStatus::Ready, &[(HOST_SHELL_OVERLAYS, "capture.overlay", 0)]),
            feature("recording", FeatureStatus::Ready, &[(HOST_SHELL_OVERLAYS, "capture.overlay", 0)]),
        ];
        assert_eq!(for_host(&features, HOST_SHELL_OVERLAYS), ["capture.overlay"]);
    }
}
