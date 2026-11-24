use crate::drawer::Drawer;
use crate::logic::{self, Logo};
use crate::tokens::WIDTH;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, AnyView, Context, Font, FontStyle, Image, ImageFormat, IntoElement, ObjectFit, ParentElement, Pixels,
    Render, SharedString, Styled, StyledImage, TextRun, Window, div, font, img, px,
};
use nova_look::tokens::QUICK_SETTINGS_PAD as PAD;
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT, BAR_MARGIN, FONT_DISPLAY, FONT_SIZE_DISPLAY, FONT_SIZE_HEADING, FONT_SIZE_SMALL, GAP,
    ICON_GAP, ICON_SIZE_MENU, ICON_SIZE_SMALL, LOGO_SIZE, PILL_BUTTON_GAP, PILL_BUTTON_INSET, QUICK_SETTINGS_GAP,
};
use nova_look::{Glyph, Icon, LookStyled, Theme, Tile};
use nova_stage::Part;
use std::sync::{Arc, LazyLock};
use std::time::Instant;

static NIXOS_LOGO: LazyLock<Arc<Image>> =
    LazyLock::new(|| Arc::new(Image::from_bytes(ImageFormat::Svg, include_bytes!("../logos/nixos.svg").to_vec())));

enum Room {
    Fits,
    Rest,
}

impl Render for Drawer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        if self.slides.iter().any(|(_, slide)| !slide.settled(now)) {
            window.request_animation_frame();
        }
        let pages = self.slides.iter().filter_map(|(page, slide)| {
            let shown = slide.value(now);
            let content = match page {
                None => self.first_page(window, cx),
                Some(page) => self.pages.get(page).map(Part::view).cloned()?.into_any_element(),
            };
            (shown > 0.0).then(|| {
                div()
                    .absolute()
                    .top_0()
                    .left(logic::page_offset(page.is_none(), shown))
                    .size_full()
                    .opacity(shown)
                    .child(content)
            })
        });
        let pages: Vec<_> = pages.collect();
        div()
            .size_full()
            .relative()
            .body_text(cx)
            .child(div().absolute().left(PAD).right(PAD).top(BAR_GAP).bottom(PAD).children(pages))
            .child(
                div()
                    .absolute()
                    .top(BAR_GAP)
                    .right(BAR_MARGIN)
                    .h(BAR_HEIGHT)
                    .px(PILL_BUTTON_INSET)
                    .flex()
                    .items_center()
                    .gap(PILL_BUTTON_GAP)
                    .children(self.buttons.views().cloned()),
            )
    }
}

impl Drawer {
    fn first_page(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let mut sections: Vec<(AnyElement, Room)> = vec![(self.header(window, cx), Room::Fits)];
        if let Some(grid) = self.tile_grid() {
            sections.push((grid.into_any_element(), Room::Fits));
        }
        let sliders = shown_views(&self.sliders);
        if !sliders.is_empty() {
            let column = div().flex().flex_col().gap(GAP).children(sliders);
            sections.push((column.into_any_element(), Room::Fits));
        }
        for card in shown_views(&self.cards) {
            sections.push((card.into_any_element(), Room::Fits));
        }
        let fill = shown_views(&self.fill).into_iter().next();
        let has_fill = fill.is_some();
        if let Some(fill) = fill {
            sections.push((fill.into_any_element(), Room::Rest));
        }
        let foot = shown_views(&self.foot).into_iter().next();

        let since = Instant::now().saturating_duration_since(self.opened_at);
        let count = sections.len() + usize::from(foot.is_some());
        if self.open && since < logic::sections_take(count) {
            window.request_animation_frame();
        }
        let shown = |order: usize| if self.open { logic::section_shown(order, since) } else { 1.0 };
        let section = |order: usize, content: AnyElement, room: Room| {
            let shown = shown(order);
            div()
                .relative()
                .top(logic::section_drop(shown))
                .opacity(shown)
                .flex_none()
                .when(matches!(room, Room::Rest), |section| section.flex_1().min_h_0().flex().flex_col())
                .child(content)
        };
        let mut children: Vec<AnyElement> = Vec::new();
        let last = sections.len();
        for (order, (content, room)) in sections.into_iter().enumerate() {
            children.push(section(order, content, room).into_any_element());
        }
        if let Some(foot) = foot {
            if !has_fill {
                children.push(div().flex_1().into_any_element());
            }
            children.push(section(last, foot.into_any_element(), Room::Fits).into_any_element());
        }
        div().size_full().flex().flex_col().gap(QUICK_SETTINGS_GAP).children(children).into_any_element()
    }

    fn tile_grid(&self) -> Option<impl IntoElement + use<>> {
        let cell = (WIDTH - PAD * 2.0 - GAP) / 2.0;
        let tiles: Vec<AnyElement> = self
            .tiles
            .slots()
            .filter(|(_, part)| part.is_none_or(Part::is_shown))
            .map(|(view, part)| match part {
                Some(part) => part.view().clone().into_any_element(),
                None => Tile::empty(SharedString::from(format!("empty {view}"))).into_any_element(),
            })
            .collect();
        if tiles.is_empty() {
            return None;
        }
        let mut rows = Vec::new();
        let mut tiles = tiles.into_iter().peekable();
        while tiles.peek().is_some() {
            let row = tiles.by_ref().take(2).map(|tile| div().w(cell).child(tile));
            rows.push(div().flex().gap(GAP).children(row));
        }
        Some(div().flex().flex_col().gap(GAP).children(rows))
    }

    fn header(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = Theme::of(cx).clone();
        let host = self.host(cx).cloned();
        let now = jiff::Zoned::now();
        let greeting = logic::greeting(now.hour());
        let name = host.as_ref().map(|host| host.user.clone()).unwrap_or_default();
        let greeting = if name.is_empty() { greeting.to_owned() } else { format!("{greeting}, ") };
        let inset = nova_look::header_inset(self.buttons.views().count());
        let room = WIDTH - PAD * 2.0 - inset - LOGO_SIZE - nova_look::tokens::PAD;
        let wide = text_width(&greeting, false, window) + text_width(&name, true, window);
        let size = logic::greeting_size(FONT_SIZE_DISPLAY, FONT_SIZE_HEADING, wide, room);
        let detail = host.as_ref().map(|host| {
            let up = jiff::Timestamp::now().as_second() - host.booted_at;
            format!("{} · {}", host.host, nova_look::span::uptime(up))
        });
        let logo = match logic::logo(host.as_ref().map(|host| &host.os)) {
            Logo::Bundled(_) => logo_image(img(NIXOS_LOGO.clone())),
            Logo::File(path) => logo_image(img(std::path::PathBuf::from(path))),
            Logo::Mark => Icon::new(Glyph::STAR).size(ICON_SIZE_MENU).color(theme.primary).into_any_element(),
        };
        div()
            .h(LOGO_SIZE)
            .mr(inset)
            .flex()
            .items_center()
            .gap(nova_look::tokens::PAD)
            .child(div().flex_none().size(LOGO_SIZE).flex().items_center().justify_center().child(logo))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .font_family(FONT_DISPLAY)
                            .text_size(size)
                            .child(div().flex_none().whitespace_nowrap().child(greeting))
                            .child(div().min_w_0().truncate().italic().text_color(theme.primary).child(name)),
                    )
                    .when_some(detail, |column, detail| {
                        column.child(
                            div()
                                .flex()
                                .items_center()
                                .gap(ICON_GAP)
                                .text_size(FONT_SIZE_SMALL)
                                .text_color(theme.fg_dim())
                                .child(Icon::new(Glyph::CLOCK).size(ICON_SIZE_SMALL).color(theme.fg_dim()))
                                .child(div().min_w_0().truncate().child(detail)),
                        )
                    }),
            )
            .into_any_element()
    }
}

fn shown_views(parts: &nova_stage::Parts) -> Vec<AnyView> {
    parts.parts().filter(|part| part.is_shown()).map(|part| part.view().clone()).collect()
}

fn logo_image(image: gpui::Img) -> AnyElement {
    image.size(LOGO_SIZE).object_fit(ObjectFit::Contain).into_any_element()
}

fn text_width(text: &str, italic: bool, window: &Window) -> Pixels {
    if text.is_empty() {
        return px(0.0);
    }
    let mut face: Font = font(FONT_DISPLAY);
    if italic {
        face.style = FontStyle::Italic;
    }
    let run = TextRun {
        len: text.len(),
        font: face,
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    window.text_system().layout_line(text, FONT_SIZE_DISPLAY, &[run], None).width
}
