use crate::logic::{self, Section};
use crate::page::BluetoothPage;
use crate::prompt::{self, card};
use crate::tokens::ROUND_BUTTON;
use crate::{PAGE, QUICK_SETTINGS};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Context, Div, InteractiveElement, IntoElement, MouseButton, ParentElement, Render, Stateful,
    StatefulInteractiveElement, Styled, Window, div,
};
use nova_bluetooth_api::{Bluetooth, BluetoothDevice, BluetoothRadio, BluetoothStatus, PairingRequest};
use nova_look::tokens::{
    DIM_OPACITY, FONT_MONO, FONT_SIZE, FONT_SIZE_SMALL, GAP, ICON_GAP, ICON_SIZE_MENU, ICON_SIZE_SMALL, LABEL_NUDGE,
    LIST_ROW_HEIGHT, PAD, QUICK_SETTINGS_GAP, RADIUS_CARD, RADIUS_ROW, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{
    CardTitle, DeviceBattery, Glyph, Icon, LookStyled, PILL_BUTTONS, PageHeader, PromptButton, RoundButton, Spinner,
    Theme, ToggleSwitch, header_inset,
};
use nova_stage::Stage;
use std::time::Instant;

impl Render for BluetoothPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        let state = self.state(cx).cloned();
        let state = state.as_ref();
        let radio = state.map_or(BluetoothRadio::Missing, |state| state.radio);
        let on = radio == BluetoothRadio::On;
        let screen = self.screen.id;
        let devices = state.map_or(&[][..], |state| state.devices.as_slice());
        let pairing = state.and_then(|state| state.pairing.as_ref()).filter(|_| on);
        let busy = devices.iter().any(|device| logic::busy_text(device.status, false).is_some());
        let scanning = on && state.is_some_and(|state| state.scanning);
        if !self.power_knob.settled(now) || (on && busy) || scanning {
            window.request_animation_frame();
        }
        self.focus_question(pairing, window, cx);

        let buttons = Stage::slots(cx).map_or(0, |slots| {
            slots.read(cx).for_host(PILL_BUTTONS, cx).iter().filter(|view| nova_stage::has_part(view, cx)).count()
        });
        let switchable = matches!(radio, BluetoothRadio::On | BluetoothRadio::Off);
        let switch = div()
            .id("power")
            .when(!switchable, |switch| switch.opacity(DIM_OPACITY))
            .when(switchable, |switch| switch.on_click(move |_, _, cx| BluetoothPage::set_power(radio, cx)))
            .child(ToggleSwitch::new(on).along(self.power_knob.value(now)));
        let header = PageHeader::new("Bluetooth", move |_, _, cx| Stage::show_page(QUICK_SETTINGS, screen, None, cx))
            .end(switch);
        let head = div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(QUICK_SETTINGS_GAP)
            .child(div().mr(header_inset(buttons)).child(header))
            .when_some(why_off(radio), |head, why| head.child(message(why, cx)));

        let unlisted = pairing.filter(|request| !devices.iter().any(|device| device.address == request.address));
        let known_shown = on || radio == BluetoothRadio::Off;
        let connected = if on { logic::in_section(devices, Section::Connected) } else { Vec::new() };
        let mine = if known_shown { logic::in_section(devices, Section::Mine) } else { Vec::new() };
        let nearby = if on { logic::in_section(devices, Section::Nearby) } else { Vec::new() };
        let lists = div()
            .flex()
            .flex_col()
            .when_some(unlisted, |lists, request| lists.child(prompt::prompt(request, &request.address, &self.pin, cx)))
            .when(!connected.is_empty(), |lists| {
                let rows = self.entries(&connected, state, cx);
                lists.child(section("Connected", false, rows, cx))
            })
            .when(!mine.is_empty(), |lists| {
                let rows = self.entries(&mine, state, cx);
                lists.child(section("My devices", false, rows, cx))
            })
            .when(on, |lists| {
                let none = nearby.is_empty() && !scanning;
                let rows = self.entries(&nearby, state, cx);
                lists.child(section("Nearby", scanning, rows, cx).when(none, |section| {
                    section.child(
                        div()
                            .pl(PAD)
                            .pt(LABEL_NUDGE * 2.0)
                            .text_size(FONT_SIZE_SMALL)
                            .text_color(Theme::of(cx).fg_dim())
                            .child("None found"),
                    )
                }))
            });
        div()
            .id(PAGE)
            .size_full()
            .body_text(cx)
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::key_down))
            .child(head)
            .child(div().id("lists").flex_1().min_h_0().mt(GAP).overflow_y_scroll().child(lists))
    }
}

fn why_off(radio: BluetoothRadio) -> Option<&'static str> {
    match radio {
        BluetoothRadio::Missing => Some("No Bluetooth adapter"),
        BluetoothRadio::SwitchedOff => Some("Blocked by a hardware switch"),
        BluetoothRadio::On | BluetoothRadio::Off => None,
    }
}

fn section(title: &'static str, spinning: bool, rows: Vec<Div>, cx: &App) -> Div {
    let label = div()
        .flex()
        .items_center()
        .justify_between()
        .pl(PAD)
        .pr(PAD)
        .pb(GAP)
        .child(CardTitle::new(title))
        .when(spinning, |label| label.child(Spinner::new().size(ICON_SIZE_SMALL).color(Theme::of(cx).fg_dim())));
    div().flex_none().flex().flex_col().pt(GAP).child(label).children(rows)
}

fn message(text: &'static str, cx: &App) -> Div {
    div()
        .p(PAD)
        .rounded(RADIUS_CARD)
        .bg(Theme::of(cx).surface_raised())
        .text_size(FONT_SIZE)
        .font_weight(WEIGHT_TITLE)
        .child(text)
}

impl BluetoothPage {
    fn focus_question(&mut self, pairing: Option<&PairingRequest>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(request) = pairing.filter(|request| logic::typed(&request.ask)) else { return };
        if self.focused_for != Some(request.id) {
            self.focused_for = Some(request.id);
            window.focus(&self.pin.read(cx).focus_handle(), cx);
        }
    }

    fn entries(&self, list: &[&BluetoothDevice], state: Option<&Bluetooth>, cx: &mut Context<Self>) -> Vec<Div> {
        list.iter().map(|device| self.entry(device, state, cx)).collect()
    }

    fn entry(&self, device: &BluetoothDevice, state: Option<&Bluetooth>, cx: &mut Context<Self>) -> Div {
        let on = state.is_some_and(|state| state.radio == BluetoothRadio::On);
        let address = device.address.clone();
        let pairing = state.and_then(|state| state.pairing.as_ref()).filter(|request| request.address == address);
        let asking = pairing.is_some();
        let failed = device.error.as_ref().is_some_and(|error| self.closed.get(&address) != Some(error));
        let opened = self.opened.as_deref() == Some(address.as_str());
        let forgetting = self.forgetting.as_deref() == Some(address.as_str());
        let mut under: Vec<AnyElement> = Vec::new();
        if on {
            if let Some(request) = pairing {
                under.push(prompt::prompt(request, &device.name, &self.pin, cx).into_any_element());
            }
            if failed && !asking {
                under.push(self.failure(device, cx).into_any_element());
            }
            if opened && device.status == BluetoothStatus::Connected {
                under.push(self.actions(&address, cx).into_any_element());
            }
            if forgetting {
                under.push(self.confirm_forget(device, cx).into_any_element());
            }
        }
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(LABEL_NUDGE * 2.0)
            .child(self.row(device, on, asking, cx))
            .children(under)
    }

    fn row(&self, device: &BluetoothDevice, on: bool, asking: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = Theme::of(cx).clone();
        let hover = theme.hover();
        let address = device.address.clone();
        let hovered = self.hovered.as_deref() == Some(address.as_str());
        let clickable = on && (logic::click(device.status).is_some() || logic::menu(device.status).is_some());
        let raised = self.opened.as_deref() == Some(address.as_str()) || asking;
        let end = self.row_end(device, asking, hovered, cx);
        let page = cx.entity();
        let (left, right, hovering) = (device.clone(), device.clone(), (page.clone(), address.clone()));
        let (page_left, page_right) = (page.clone(), page);
        div()
            .id(("device", crate::hash(&address)))
            .flex_none()
            .h(LIST_ROW_HEIGHT)
            .rounded(RADIUS_ROW)
            .flex()
            .items_center()
            .pl(PAD)
            .pr(PAD)
            .when(!on, |row| row.opacity(DIM_OPACITY))
            .when(raised, |row| row.bg(theme.surface_raised()))
            .on_hover(move |hovered, _, cx| {
                let (page, address) = &hovering;
                page.update(cx, |page, cx| page.hover(address, *hovered, cx));
            })
            .when(clickable, |row| {
                row.hover(move |style| style.bg(hover))
                    .on_click(move |_, _, cx| page_left.update(cx, |page, cx| page.act(&left, cx)))
                    .on_mouse_down(MouseButton::Right, move |_, _, cx| {
                        page_right.update(cx, |page, cx| page.open_menu(&right, cx))
                    })
            })
            .child(Icon::new(logic::icon(device.kind)).size(ICON_SIZE_MENU))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .ml(PAD)
                    .mr(GAP)
                    .truncate()
                    .text_size(FONT_SIZE)
                    .font_weight(WEIGHT_LABEL)
                    .child(device.name.clone()),
            )
            .when(on, |row| row.child(div().flex_none().flex().items_center().gap(GAP).children(end)))
    }

    fn row_end(
        &self,
        device: &BluetoothDevice,
        asking: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let theme = Theme::of(cx).clone();
        let address = device.address.clone();
        let busy = logic::busy_text(device.status, asking);
        let page = cx.entity();
        let words = busy.or_else(|| hovered.then(|| logic::hover_text(device.status)).flatten());
        let mut end: Vec<AnyElement> = Vec::new();
        if let Some(words) = words {
            let ink = if busy.is_some() { theme.primary } else { theme.fg_dim() };
            end.push(div().text_size(FONT_SIZE_SMALL).text_color(ink).child(words).into_any_element());
        }
        if busy.is_some() {
            end.push(Spinner::new().size(ICON_SIZE_SMALL).color(theme.primary).into_any_element());
        }
        let stoppable = matches!(device.status, BluetoothStatus::Connecting | BluetoothStatus::Pairing);
        if !asking && stoppable {
            let (page, address) = (page.clone(), address.clone());
            end.push(
                RoundButton::new("stop", Glyph::CLOSE, ROUND_BUTTON)
                    .quiet()
                    .build(cx, move |_, _, cx| page.update(cx, |page, cx| page.disconnect(address.clone(), cx)))
                    .into_any_element(),
            );
        }
        if let Some(percent) = device.battery.filter(|_| busy.is_none() && device.status == BluetoothStatus::Connected)
        {
            end.push(
                div()
                    .flex()
                    .items_center()
                    .gap(ICON_GAP)
                    .child(
                        DeviceBattery::percent(percent)
                            .eased(gpui::SharedString::from(format!("battery {}", device.address))),
                    )
                    .child(
                        div()
                            .font_family(FONT_MONO)
                            .text_size(FONT_SIZE_SMALL)
                            .text_color(theme.fg_dim())
                            .child(format!("{percent}%")),
                    )
                    .into_any_element(),
            );
        }
        if busy.is_none() && device.status == BluetoothStatus::Paired {
            let (page, address) = (page.clone(), address.clone());
            let forgetting = self.forgetting.as_deref() == Some(address.as_str());
            end.push(
                RoundButton::new("more", Glyph::DOTS, ROUND_BUTTON)
                    .quiet()
                    .on(forgetting)
                    .build(cx, move |_, _, cx| page.update(cx, |page, cx| page.toggle_forget(&address, cx)))
                    .into_any_element(),
            );
        }
        end
    }

    fn failure(&self, device: &BluetoothDevice, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let page = cx.entity();
        let (closed, retried) = (device.clone(), device.clone());
        let close = {
            let page = page.clone();
            PromptButton::new(("close", crate::hash(&device.address)), "Close")
                .on_click(move |_, _, cx| page.update(cx, |page, cx| page.close_failure(&closed, cx)))
        };
        let mut buttons = vec![close.into_any_element()];
        if device.status != BluetoothStatus::Connected {
            buttons.push(
                PromptButton::new(("retry", crate::hash(&device.address)), "Try again")
                    .accent(true)
                    .on_click(move |_, _, cx| page.update(cx, |page, cx| page.act(&retried, cx)))
                    .into_any_element(),
            );
        }
        card(theme.error, device.error.as_deref().unwrap_or(""), cx).child(prompt::buttons(buttons))
    }

    fn actions(&self, address: &str, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let page = cx.entity();
        let (gone, forgotten) = (address.to_owned(), address.to_owned());
        let disconnect = {
            let page = page.clone();
            PromptButton::new("disconnect", "Disconnect")
                .on_click(move |_, _, cx| page.update(cx, |page, cx| page.disconnect(gone.clone(), cx)))
        };
        let forget = PromptButton::new("forget", "Forget")
            .on_click(move |_, _, cx| page.update(cx, |page, cx| page.forget(forgotten.clone(), cx)));
        card(theme.fg, "", cx).child(prompt::buttons(vec![disconnect.into_any_element(), forget.into_any_element()]))
    }

    fn confirm_forget(&self, device: &BluetoothDevice, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let page = cx.entity();
        let cancel = {
            let page = page.clone();
            PromptButton::new("cancel", "Cancel").on_click(move |_, _, cx| {
                page.update(cx, |page, cx| {
                    page.forgetting = None;
                    cx.notify();
                })
            })
        };
        let address = device.address.clone();
        let forget = PromptButton::new("forget-confirm", "Forget")
            .accent(true)
            .on_click(move |_, _, cx| page.update(cx, |page, cx| page.forget(address.clone(), cx)));
        card(theme.fg, &format!("Forget {}?", device.name), cx)
            .child(prompt::buttons(vec![cancel.into_any_element(), forget.into_any_element()]))
    }
}
