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
use nova_look::{AppIcon, LookStyled, PageHeader, Theme, ToggleSwitch, wheel_list};
use nova_stage::Stage;

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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let soft = Theme::of(cx).fg_soft();
        let list = self.model.read(cx).list(cx).cloned();
        let do_not_disturb = list.as_ref().is_some_and(|list| list.do_not_disturb);
        let screen = self.screen;
        let header =
            PageHeader::new("Notifications", move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, None, cx));
        let apps: Vec<AnyElement> = list
            .as_ref()
            .map(|list| {
                list.apps
                    .iter()
                    .enumerate()
                    .map(|(at, app)| {
                        let muted = list.muted.contains(&app.name);
                        let icon =
                            AppIcon::new(app.app_id.clone().unwrap_or_else(|| app.name.clone()), app.name.clone())
                                .size(ROW_ICON)
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
        let apps = (!apps.is_empty())
            .then(|| wheel_list("apps", window, cx).flex_1().min_h_0().flex().flex_col().children(apps));
        let dnd = SwitchRow { lead: None, title: "Do not disturb".into(), on: do_not_disturb, selected: true }.build(
            "do-not-disturb",
            cx,
            move |_, _, cx| model::set_do_not_disturb(!do_not_disturb, cx),
        );
        div().size_full().body_text(cx).flex().flex_col().gap(QUICK_SETTINGS_GAP).child(header).child(dnd).when_some(
            apps,
            |page, apps| {
                page.child(div().px(PAD).text_size(FONT_SIZE_SMALL).text_color(soft).child("APPS")).child(apps)
            },
        )
    }
}

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
