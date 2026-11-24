use crate::logic::{self, LinkIcon, Role};
use crate::page::NetworkPage;
use crate::tokens::{LEGEND_DOT, MENU_WIDTH, MORE_SIZE, PASSWORD_HEIGHT, TRAFFIC_GRAPH_HEIGHT, WIFI_ARC_FADED_OPACITY};
use crate::{PAGE, QUICK_SETTINGS};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Anchor, AnyElement, App, Bounds, Context, Div, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, Render, SharedString, Stateful, StatefulInteractiveElement, Styled, Window, anchored, canvas, deferred,
    div, fill, point, size,
};
use nova_look::tokens::{
    CHIP_HEIGHT, CHIP_PAD, DIM_OPACITY, DIVIDER_WIDTH, FONT_MONO, FONT_SIZE, FONT_SIZE_MICRO, FONT_SIZE_SMALL,
    FONT_SIZE_TITLE, GAP, ICON_GAP, ICON_SIZE_MENU, ICON_SIZE_SMALL, LABEL_NUDGE, LINE_HEIGHT, LIST_ROW_HEIGHT, PAD,
    QUICK_SETTINGS_GAP, RADIUS_CARD, RADIUS_CONTROL, RADIUS_ROW, RADIUS_SEGMENT, ROW_INSET, SEGMENT_HEIGHT,
    SEGMENT_PAD, WEIGHT_BODY, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{
    CardTitle, Glyph, Icon, LookStyled, PageHeader, PromptButton, RoundButton, Segments, Spinner, Theme, ToggleSwitch,
    graph,
};
use nova_network_api::{LinkKind, LinkState, NETWORK_TRAFFIC_HISTORY_SECS, Network, NetworkTraffic, WifiNetwork};
use nova_stage::Stage;
use std::time::Instant;

impl Render for NetworkPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if std::mem::take(&mut self.focus_password) && self.asked.is_some() {
            window.focus(&self.password_focus(cx), cx);
        }
        let network = self.network(cx).cloned();
        let network = network.as_ref();
        let traffic = self.traffic.read(cx).get().cloned();
        let screen = self.screen.id;
        let scanning = network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.scanning);
        if !self.wifi_knob.settled(now) || logic::connecting(network).is_some() || scanning {
            window.request_animation_frame();
        }
        let rows_above = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(QUICK_SETTINGS_GAP)
            .child(PageHeader::new("Network", move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, None, cx)))
            .child(traffic_card(network, traffic.as_ref(), cx))
            .when(logic::prefer_shown(network), |rows| rows.child(prefer(network)))
            .when_some(logic::wired_link(network), |rows, link| {
                let connected = link.state == LinkState::Connected;
                let glyph = if connected { Glyph::WIRED } else { Glyph::WIRED_OFF };
                let row = LinkRow::new("wired", "Wired", Icon::new(glyph).size(ICON_SIZE_MENU))
                    .subtitle(logic::wired_line(link))
                    .role(connected.then(|| logic::role(network, LinkKind::Wired)))
                    .selected(connected);
                rows.child(row.build(cx))
            })
            .when(network.is_some_and(|network| network.wifi.is_some()), |rows| {
                rows.child(self.wifi_row(network, now, cx))
            });
        let shown_list = logic::wifi_on(network);
        let menu_open = self.menu_for.is_some();
        div()
            .id(PAGE)
            .size_full()
            .relative()
            .body_text(cx)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::key_down))
            .child(rows_above)
            .when(shown_list, |page| page.child(self.list(network, window, cx)))
            .when(menu_open, |page| {
                page.child(
                    div()
                        .id("beside-menu")
                        .absolute()
                        .inset_0()
                        .occlude()
                        .on_mouse_down(MouseButton::Left, cx.listener(|page, _, _, cx| page.close_menu(cx))),
                )
            })
    }
}

impl NetworkPage {
    fn wifi_row(&self, network: Option<&Network>, now: Instant, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = logic::wifi_on(network);
        let blocked = network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.blocked);
        let glyph = if on { Glyph::WIFI } else { Glyph::WIFI_OFF };
        let switch = div()
            .when(blocked, |switch| switch.opacity(DIM_OPACITY))
            .child(ToggleSwitch::new(on).along(self.wifi_knob.value(now)));
        let enabled = network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.enabled);
        LinkRow::new("wifi-switch", "Wi-Fi", Icon::new(glyph).filled(Glyph::WIFI_DOT).size(ICON_SIZE_MENU))
            .end(switch)
            .on_click(move |cx| NetworkPage::set_wifi(enabled, blocked, cx))
            .build(cx)
    }

    fn list(&self, network: Option<&Network>, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::of(cx).clone();
        let current = logic::current(network);
        let others = logic::others(network);
        let scanning = network.and_then(|network| network.wifi.as_ref()).is_some_and(|wifi| wifi.scanning);
        let title = if current.is_some() { "Other networks" } else { "Networks" };
        div()
            .id("networks")
            .flex_1()
            .min_h_0()
            .mt(GAP)
            .overflow_y_scroll()
            .on_scroll(cx.listener(|page, _, _, cx| page.close_menu(cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(current.map(|net| self.entry(network, net, window, cx)))
                    .when(!others.is_empty(), |list| {
                        list.child(div().pl(PAD).pt(GAP).pb(LABEL_NUDGE * 2.0).child(CardTitle::new(title)))
                    })
                    .children(others.into_iter().map(|net| self.entry(network, net, window, cx)))
                    .when(scanning, |list| {
                        list.child(
                            div()
                                .h(LIST_ROW_HEIGHT)
                                .flex_none()
                                .flex()
                                .items_center()
                                .pl(PAD)
                                .gap(PAD)
                                .child(Spinner::new().size(ICON_SIZE_MENU).color(theme.fg_dim()))
                                .child(div().text_size(FONT_SIZE).text_color(theme.fg_dim()).child("Scanning…")),
                        )
                    }),
            )
    }

    fn entry(&self, network: Option<&Network>, net: &WifiNetwork, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let ssid: SharedString = net.ssid.clone().into();
        let joinable = logic::joinable(net);
        let joining = logic::connecting(network) == Some(net.ssid.as_str());
        let has_menu = (net.saved || net.connected) && !joining;
        let menu_open = has_menu && self.menu_for.as_deref() == Some(net.ssid.as_str());
        let asking = self.asked.as_deref() == Some(net.ssid.as_str());
        let group: SharedString = format!("network-{ssid}").into();
        let mut end: Vec<AnyElement> = Vec::new();
        if joining {
            end.push(Spinner::new().size(ICON_SIZE_SMALL).into_any_element());
        } else if logic::needs_password(net) {
            end.push(Icon::new(Glyph::LOCK).size(ICON_SIZE_SMALL).color(theme.fg_dim()).into_any_element());
        }
        if has_menu {
            let entity = cx.entity();
            let toggle = {
                let ssid = net.ssid.clone();
                move |_: &_, _: &mut Window, cx: &mut App| entity.update(cx, |page, cx| page.toggle_menu(&ssid, cx))
            };
            let more =
                RoundButton::new(("more", hash(&ssid)), Glyph::DOTS, MORE_SIZE).filled().quiet().build(cx, toggle);
            let more = match menu_open || net.connected {
                true => more,
                false => more.opacity(0.0).group_hover(group.clone(), |style| style.opacity(1.0)),
            };
            end.push(more.into_any_element());
        }
        let entity = cx.entity();
        let picked = net.clone();
        let failed = logic::failed(network, &net.ssid);
        let row = LinkRow::new(("network", hash(&ssid)), ssid.clone(), wifi_icon(net.strength, theme.fg))
            .subtitle(logic::network_line(network, net))
            .subtitle_color(if failed { theme.error } else { theme.fg_dim() })
            .role(net.connected.then(|| logic::role(network, LinkKind::Wifi)))
            .selected(net.connected)
            .clickable(joinable)
            .group(group)
            .ends(end)
            .on_click_window(move |window, cx| entity.update(cx, |page, cx| page.pick(&picked, window, cx)))
            .build(cx)
            .when(!joinable, |row| row.opacity(DIM_OPACITY));
        div()
            .relative()
            .flex_none()
            .flex()
            .flex_col()
            .child(row)
            .when(asking, |entry| entry.child(self.password_field(window, cx)))
            .when(menu_open, |entry| entry.child(self.menu(net, cx)))
    }

    fn password_field(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let focused = self.password_focus(cx).is_focused(window);
        let border = match (self.wrong, focused) {
            (true, _) => theme.error,
            (false, true) => theme.primary,
            (false, false) => theme.outline(),
        };
        let fits = logic::password_fits(self.password.read(cx).text());
        let eye = if self.show_password { Glyph::EYE_OFF } else { Glyph::EYE };
        let page = cx.entity();
        let field = div()
            .mx(PAD)
            .h(PASSWORD_HEIGHT)
            .flex_none()
            .rounded_full()
            .bg(theme.track())
            .border(DIVIDER_WIDTH)
            .border_color(border)
            .flex()
            .items_center()
            .pl(PAD - DIVIDER_WIDTH)
            .pr(LABEL_NUDGE * 2.0 - DIVIDER_WIDTH)
            .gap(GAP)
            .child(Icon::new(Glyph::LOCK).size(ICON_SIZE_SMALL).color(theme.fg_dim()))
            .child(div().flex_1().min_w_0().text_size(FONT_SIZE_SMALL).child(self.password.clone()))
            .child(RoundButton::new("eye", eye, MORE_SIZE).quiet().build(cx, {
                let page = page.clone();
                move |_, _, cx| page.update(cx, |page, cx| page.toggle_shown(cx))
            }));
        let buttons = div()
            .flex()
            .justify_end()
            .items_center()
            .mr(PAD)
            .gap(ICON_GAP)
            .when(self.wrong, |row| {
                row.child(div().pr(GAP).text_size(FONT_SIZE_SMALL).text_color(theme.error).child("Wrong password"))
            })
            .child(PromptButton::new("cancel", "Cancel").on_click({
                let page = page.clone();
                move |_, window, cx| page.update(cx, |page, cx| page.cancel(window, cx))
            }))
            .child(
                PromptButton::new("connect", "Connect")
                    .accent(true)
                    .enabled(fits)
                    .on_click(move |_, window, cx| page.update(cx, |page, cx| page.submit(window, cx))),
            );
        div().flex().flex_col().pt(LABEL_NUDGE * 2.0).pb(GAP).gap(GAP).child(field).child(buttons)
    }

    fn menu(&self, net: &WifiNetwork, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let page = cx.entity();
        let mut items: Vec<AnyElement> = Vec::new();
        if net.connected {
            let page = page.clone();
            items.push(
                menu_item("disconnect", Glyph::CLOSE, "Disconnect", &theme)
                    .on_click(move |_, _, cx| page.update(cx, |page, cx| page.disconnect(cx)))
                    .into_any_element(),
            );
        }
        if net.saved {
            let ssid = net.ssid.clone();
            items.push(
                menu_item("forget", Glyph::TRASH, "Forget", &theme)
                    .on_click(move |_, _, cx| page.update(cx, |page, cx| page.forget(ssid.clone(), cx)))
                    .into_any_element(),
            );
        }
        let menu = div()
            .id("menu")
            .occlude()
            .w(MENU_WIDTH)
            .p(SEGMENT_PAD)
            .rounded(RADIUS_CONTROL)
            .bg(theme.bg)
            .border(DIVIDER_WIDTH)
            .border_color(theme.glass_rim())
            .flex()
            .flex_col()
            .body_text(cx)
            .children(items);
        div()
            .absolute()
            .top((LIST_ROW_HEIGHT + MORE_SIZE) / 2.0 + LABEL_NUDGE)
            .right(PAD)
            .size_0()
            .child(deferred(anchored().anchor(Anchor::TopRight).child(menu)))
    }
}

fn hash(ssid: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    ssid.hash(&mut hasher);
    hasher.finish()
}

fn menu_item(id: &'static str, glyph: Glyph, label: &'static str, theme: &Theme) -> Stateful<Div> {
    let hover = theme.hover();
    div()
        .id(id)
        .h(SEGMENT_HEIGHT)
        .flex_none()
        .rounded(RADIUS_SEGMENT)
        .flex()
        .items_center()
        .px(ROW_INSET)
        .gap(GAP)
        .hover(move |style| style.bg(hover))
        .child(Icon::new(glyph).size(ICON_SIZE_SMALL))
        .child(div().flex_1().min_w_0().truncate().text_size(FONT_SIZE_SMALL).child(label))
}

fn traffic_card(network: Option<&Network>, traffic: Option<&NetworkTraffic>, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let online = network.is_some_and(|network| network.primary.is_some());
    let mbps = logic::link_mbps(network);
    let link = div()
        .flex()
        .items_center()
        .gap(GAP)
        .child(link_icon(logic::card_icon(network), ICON_SIZE_SMALL, theme.fg))
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(FONT_SIZE_SMALL)
                .font_weight(WEIGHT_TITLE)
                .child(logic::link_name(network)),
        )
        .when(mbps > 0, |link| {
            link.child(
                div()
                    .flex_none()
                    .text_size(FONT_SIZE_SMALL)
                    .text_color(theme.fg_dim())
                    .child(format!("{} link", logic::link_speed(mbps))),
            )
        });
    let (down, up) = traffic.map_or((0, 0), |traffic| (logic::now(&traffic.down), logic::now(&traffic.up)));
    let speeds = div().flex().gap(PAD).child(speed(theme.primary, "↓", down, theme.fg)).child(speed(
        theme.secondary,
        "↑",
        up,
        theme.fg,
    ));
    let today = traffic.map(logic::today).unwrap_or_default();
    div()
        .flex_none()
        .p(PAD)
        .rounded(RADIUS_CARD)
        .bg(theme.surface_raised())
        .flex()
        .flex_col()
        .gap(GAP)
        .child(link)
        .when(online, |card| card.child(speeds))
        .child(traffic_graph(traffic.filter(|_| online).cloned(), theme))
        .child(
            div()
                .flex()
                .justify_end()
                .min_h(FONT_SIZE_MICRO * LINE_HEIGHT)
                .font_family(FONT_MONO)
                .text_size(FONT_SIZE_MICRO)
                .text_color(theme.fg_dim())
                .child(today),
        )
}

fn speed(color: Hsla, arrow: &str, bytes_per_second: u64, ink: Hsla) -> Div {
    div().flex().items_center().gap(ICON_GAP).child(div().size(LEGEND_DOT).flex_none().rounded_full().bg(color)).child(
        div()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE_TITLE)
            .text_color(ink)
            .child(format!("{arrow} {}", logic::speed(bytes_per_second))),
    )
}

fn traffic_graph(traffic: Option<NetworkTraffic>, theme: &Theme) -> impl IntoElement {
    let (outline, down_color, up_color) = (theme.outline(), theme.primary, theme.secondary);
    let span = NETWORK_TRAFFIC_HISTORY_SECS as usize;
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let middle = point(bounds.left(), bounds.top() + bounds.size.height / 2.0);
            window.paint_quad(fill(Bounds::new(middle, size(bounds.size.width, DIVIDER_WIDTH)), outline));
            let Some(traffic) = &traffic else { return };
            let peak = logic::peak(traffic);
            for (values, color) in [(&traffic.down, down_color), (&traffic.up, up_color)] {
                let points = graph::points(&logic::scaled(values, peak), bounds, span);
                graph::line(&points, color, window);
            }
        },
    )
    .w_full()
    .h(TRAFFIC_GRAPH_HEIGHT)
}

fn link_icon(icon: LinkIcon, size: Pixels, color: Hsla) -> Icon {
    let drawn = Icon::new(icon.glyph).size(size).color(color);
    match icon.fill {
        Some(fill) => drawn.filled(fill),
        None => drawn,
    }
}

fn wifi_icon(strength: u8, color: Hsla) -> AnyElement {
    let lit = logic::arcs(strength);
    let arcs = [Glyph::WIFI_ARC_INNER, Glyph::WIFI_ARC_MIDDLE, Glyph::WIFI_ARC_OUTER];
    let layers = arcs.into_iter().enumerate().map(|(index, arc)| {
        let icon = Icon::new(arc).size(ICON_SIZE_MENU);
        let icon = match index < lit {
            true => icon.color(color),
            false => icon.color(color.alpha(WIFI_ARC_FADED_OPACITY)),
        };
        let icon = if index == 0 { icon.filled(Glyph::WIFI_DOT) } else { icon };
        div().absolute().inset_0().child(icon)
    });
    div().relative().size(ICON_SIZE_MENU).flex_none().children(layers).into_any_element()
}

fn prefer(network: Option<&Network>) -> Div {
    let current = match network.and_then(|network| network.prefer) {
        Some(LinkKind::Wired) => "wired",
        Some(LinkKind::Wifi) => "wifi",
        _ => "",
    };
    let segments = Segments::new([("wired", "Wired"), ("wifi", "Wi-Fi")], current)
        .accent(true)
        .icon("wired", Glyph::WIRED, None)
        .icon("wifi", Glyph::WIFI, Some(Glyph::WIFI_DOT))
        .on_pick(|key, _, cx| {
            let kind = if key == "wired" { LinkKind::Wired } else { LinkKind::Wifi };
            NetworkPage::prefer(kind, cx);
        });
    div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(GAP)
        .child(div().ml(LABEL_NUDGE * 2.0).child(CardTitle::new("Prefer")))
        .child(segments)
}

type RowClicked = Box<dyn Fn(&mut Window, &mut App)>;

struct LinkRow {
    id: gpui::ElementId,
    title: SharedString,
    lead: AnyElement,
    subtitle: String,
    subtitle_color: Option<Hsla>,
    role: Option<Role>,
    selected: bool,
    clickable: bool,
    group: Option<SharedString>,
    end: Vec<AnyElement>,
    on_click: Option<RowClicked>,
}

impl LinkRow {
    fn new(id: impl Into<gpui::ElementId>, title: impl Into<SharedString>, lead: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            lead: lead.into_any_element(),
            subtitle: String::new(),
            subtitle_color: None,
            role: None,
            selected: false,
            clickable: true,
            group: None,
            end: Vec::new(),
            on_click: None,
        }
    }

    fn subtitle(mut self, subtitle: String) -> Self {
        self.subtitle = subtitle;
        self
    }

    fn subtitle_color(mut self, color: Hsla) -> Self {
        self.subtitle_color = Some(color);
        self
    }

    fn role(mut self, role: Option<Role>) -> Self {
        self.role = role;
        self
    }

    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn clickable(mut self, clickable: bool) -> Self {
        self.clickable = clickable;
        self
    }

    fn group(mut self, group: SharedString) -> Self {
        self.group = Some(group);
        self
    }

    fn end(mut self, end: impl IntoElement) -> Self {
        self.end.push(end.into_any_element());
        self
    }

    fn ends(mut self, end: Vec<AnyElement>) -> Self {
        self.end.extend(end);
        self
    }

    fn on_click(self, clicked: impl Fn(&mut App) + 'static) -> Self {
        self.on_click_window(move |_, cx| clicked(cx))
    }

    fn on_click_window(mut self, clicked: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(clicked));
        self
    }

    fn build<V>(self, cx: &Context<V>) -> Stateful<Div> {
        let theme = Theme::of(cx);
        let hover = theme.hover();
        let chip = self.role.map(|role| {
            let in_use = role != Role::Backup;
            div()
                .flex_none()
                .h(CHIP_HEIGHT)
                .px(CHIP_PAD)
                .rounded_full()
                .flex()
                .items_center()
                .bg(if in_use { theme.primary } else { theme.track() })
                .text_size(FONT_SIZE_SMALL)
                .font_weight(if in_use { WEIGHT_TITLE } else { WEIGHT_BODY })
                .text_color(if in_use { theme.on_accent } else { theme.fg_soft() })
                .child(role.label())
        });
        let clickable = self.clickable;
        div()
            .id(self.id)
            .flex_none()
            .w_full()
            .h(LIST_ROW_HEIGHT)
            .rounded(RADIUS_ROW)
            .flex()
            .items_center()
            .pl(PAD)
            .pr(PAD)
            .when_some(self.group, |row, group| row.group(group))
            .when(self.selected, |row| row.bg(theme.surface_raised()))
            .when(clickable, |row| row.hover(move |style| style.bg(hover)))
            .when_some(self.on_click.filter(|_| clickable), |row, clicked| {
                row.on_click(move |_, window, cx| clicked(window, cx))
            })
            .child(div().flex_none().w(ICON_SIZE_MENU).flex().justify_center().child(self.lead))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .ml(PAD)
                    .mr(GAP)
                    .flex()
                    .flex_col()
                    .child(div().truncate().text_size(FONT_SIZE).font_weight(WEIGHT_LABEL).child(self.title))
                    .when(!self.subtitle.is_empty(), |text| {
                        text.child(
                            div()
                                .truncate()
                                .text_size(FONT_SIZE_SMALL)
                                .text_color(self.subtitle_color.unwrap_or(theme.fg_dim()))
                                .child(self.subtitle),
                        )
                    }),
            )
            .child(div().flex_none().flex().items_center().gap(GAP).children(chip).children(self.end))
    }
}
