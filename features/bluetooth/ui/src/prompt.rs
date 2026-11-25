use crate::logic;
use crate::tokens::{
    PAIRING_CELL_GAP, PAIRING_CELL_HEIGHT, PAIRING_CELL_WIDTH, PAIRING_FIELD_HEIGHT, PAIRING_TINT_OPACITY,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{AnyElement, App, Div, Entity, Hsla, ParentElement, SharedString, Styled, div};
use nova_bluetooth_api::{Command, PairingAnswer, PairingRequest};
use nova_look::tokens::{
    FONT_MONO, FONT_SIZE, FONT_SIZE_SMALL, FONT_SIZE_TITLE, GAP, ICON_GAP, PAD, RADIUS_ROW, RADIUS_SEGMENT,
    WEIGHT_TITLE,
};
use nova_look::{PromptButton, TextField, Theme};
use nova_ui_client::Daemon;

pub fn accept(request: &PairingRequest, field: &Entity<TextField>, cx: &mut App) {
    let typed = field.read(cx).text().to_owned();
    if logic::typed(&request.ask) {
        if logic::valid_code(&request.ask, &typed) {
            answer(request.id, PairingAnswer::Code(typed.trim().to_owned()), cx);
        }
        return;
    }
    answer(request.id, PairingAnswer::Accept, cx);
}

pub fn answer(id: u64, answer: PairingAnswer, cx: &mut App) {
    Daemon::send(Command::BluetoothAnswer { id, answer }, cx);
}

pub fn prompt(request: &PairingRequest, name: &str, field: &Entity<TextField>, cx: &App) -> Div {
    let theme = Theme::of(cx);
    let ask = &request.ask;
    let (decline, yes) = logic::buttons(ask, request.incoming);
    let typed = logic::typed(ask);
    let can_accept = !typed || logic::valid_code(ask, field.read(cx).text());
    let code = logic::code(ask).map(|code| {
        let done = logic::typed_on_device(ask);
        let cells = code.chars().enumerate().map(|(index, character)| {
            let lit = index < done;
            div()
                .w(PAIRING_CELL_WIDTH)
                .h(PAIRING_CELL_HEIGHT)
                .flex_none()
                .rounded(RADIUS_SEGMENT)
                .flex()
                .items_center()
                .justify_center()
                .bg(if lit { theme.primary } else { theme.track() })
                .font_family(FONT_MONO)
                .text_size(FONT_SIZE_TITLE)
                .font_weight(WEIGHT_TITLE)
                .text_color(if lit { theme.on_accent } else { theme.fg })
                .child(SharedString::from(character.to_string()))
        });
        div().flex().gap(PAIRING_CELL_GAP).children(cells)
    });
    let input = typed.then(|| {
        div()
            .h(PAIRING_FIELD_HEIGHT)
            .px(PAD)
            .rounded(RADIUS_SEGMENT)
            .bg(theme.track())
            .flex()
            .items_center()
            .font_family(FONT_MONO)
            .text_size(FONT_SIZE)
            .child(field.clone())
    });
    let (id, accepted, field) = (request.id, request.clone(), field.clone());
    let buttons = div()
        .flex()
        .justify_end()
        .gap(ICON_GAP)
        .child(PromptButton::new("decline", decline).on_click(move |_, _, cx| answer(id, PairingAnswer::Reject, cx)))
        .when_some(yes, |row, yes| {
            row.child(
                PromptButton::new("accept", yes)
                    .accent(true)
                    .enabled(can_accept)
                    .on_click(move |_, _, cx| accept(&accepted, &field, cx)),
            )
        });
    card(theme.primary, &logic::question(ask, name, request.incoming), cx).children(code).children(input).child(buttons)
}

pub fn card(tone: Hsla, text: &str, cx: &App) -> Div {
    let theme = Theme::of(cx);
    div()
        .flex_none()
        .p(PAD)
        .rounded(RADIUS_ROW)
        .bg(tone.alpha(PAIRING_TINT_OPACITY))
        .flex()
        .flex_col()
        .gap(GAP)
        .when(!text.is_empty(), |card| {
            card.child(div().text_size(FONT_SIZE_SMALL).text_color(theme.fg).child(text.to_owned()))
        })
}

pub fn buttons(buttons: Vec<AnyElement>) -> Div {
    div().flex().justify_end().gap(ICON_GAP).children(buttons)
}
