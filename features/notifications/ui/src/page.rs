//! The notifications' settings page (`notifications.page`): do not
//! disturb, and a switch per app that has sent one (off mutes it).
//! `ui/quick_settings/NotificationSettingsPage.qml` in today's shell.
//!
//! Quick settings draws it; the list's gear opens it and its back button
//! goes home (`Stage::show_page`).

use crate::app_icon::AppIcon;
use crate::model::{self, Model};
use crate::tokens::ROW_ICON;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, ClickEvent, Context, DisplayId, ElementId, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, div,
};
use nova_look::tokens::{
    FONT_SIZE, FONT_SIZE_SMALL, LIST_ROW_HEIGHT, PAD, QUICK_SETTINGS_GAP, RADIUS_ROW, WEIGHT_LABEL,
};
use nova_look::{LookStyled, PageHeader, Theme, ToggleSwitch};
use nova_stage::Stage;

/// The panel it is a page of.
const QUICK_SETTINGS: &str = "quick-settings";

pub struct Page {
    screen: DisplayId,
    model: Entity<Model>,
}

impl Page {
    pub fn new(screen: DisplayId, cx: &mut Context<Self>) -> Self {
        let model = Model::get(cx);
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        Self { screen, model }
    }
}

impl Render for Page {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::of(cx);
        let list = self.model.read(cx).list(cx).cloned();
        let do_not_disturb = list.as_ref().is_some_and(|list| list.do_not_disturb);
        let screen = self.screen;
        let header =
            PageHeader::new("Notifications", move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, None, cx));
        let dnd = SwitchRow { lead: None, title: "Do not disturb".into(), on: do_not_disturb, selected: true }.build(
            "do-not-disturb",
            cx,
            move |_, _, cx| model::set_do_not_disturb(!do_not_disturb, cx),
        );
        let apps: Vec<AnyElement> = list
            .as_ref()
            .map(|list| {
                list.apps
                    .iter()
                    .enumerate()
                    .map(|(at, app)| {
                        let muted = list.muted.contains(&app.name);
                        let icon = AppIcon::new(
                            app.app_id.clone().unwrap_or_else(|| app.name.clone()),
                            app.name.clone(),
                            ROW_ICON,
                        )
                        .icon(app.icon.as_deref())
                        .into_any_element();
                        let name = app.name.clone();
                        SwitchRow { lead: Some(icon), title: app.name.clone(), on: !muted, selected: false }
                            .build(("app", at), cx, move |_, _, cx| model::mute(&name, !muted, cx))
                            .into_any_element()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let has_apps = !apps.is_empty();
        div().size_full().body_text(cx).flex().flex_col().gap(QUICK_SETTINGS_GAP).child(header).child(dnd).when(
            has_apps,
            |page| {
                page.child(div().px(PAD).text_size(FONT_SIZE_SMALL).text_color(theme.fg_soft()).child("APPS"))
                    .child(div().id("apps").flex_1().min_h_0().overflow_y_scroll().flex().flex_col().children(apps))
            },
        )
    }
}

/// A row with a name and a switch at its end, and an icon before the name
/// when there is one; `selected` rows sit on a raised surface.
struct SwitchRow {
    lead: Option<AnyElement>,
    title: String,
    on: bool,
    selected: bool,
}

impl SwitchRow {
    fn build(
        self,
        id: impl Into<ElementId>,
        cx: &App,
        clicked: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        let Self { lead, title, on, selected } = self;
        let theme = Theme::of(cx);
        let hover = theme.hover();
        div()
            .id(id)
            .flex_none()
            .h(LIST_ROW_HEIGHT)
            .px(PAD)
            .flex()
            .items_center()
            .gap(PAD)
            .rounded(RADIUS_ROW)
            .when(selected, |row| row.bg(theme.surface_raised()))
            .hover(move |style| style.bg(hover))
            .cursor_pointer()
            .on_click(clicked)
            .children(
                lead.map(|lead| div().flex_none().size(ROW_ICON).flex().items_center().justify_center().child(lead)),
            )
            .child(div().flex_1().min_w_0().truncate().text_size(FONT_SIZE).font_weight(WEIGHT_LABEL).child(title))
            .child(ToggleSwitch::new(on))
    }
}
