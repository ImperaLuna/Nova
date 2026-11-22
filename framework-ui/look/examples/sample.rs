//! A pill and a panel drawn with the whole look kit, to look at and to
//! screenshot beside today's shell: glass with its edge and shadow, text at
//! each size and ink, icons, fills, a meter that eases in, the theme's
//! colors, and a footer held inside the panel's rounded corners. The blur
//! and click regions follow the two shapes.
//!
//! It opens one shell window across the top of the first screen. A click on
//! the panel swaps dark and light, which shows that a theme change redraws
//! everything with nothing watching for it.
//!
//! `cargo run -p nova-look --example sample -- --help` lists its switches.

use gpui::{
    App, AppContext, Bounds, Context, Div, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render,
    Styled, Window, div, point, px, relative, size,
};
use gpui_platform::application;
use nova_look::motion::Eased;
use nova_look::tokens::{
    BAR_GAP, BAR_HEIGHT, CHIP_HEIGHT, CHIP_PAD, DIVIDER_HEIGHT, DIVIDER_WIDTH, DURATION_FILL, FONT_MONO,
    FONT_SIZE_HEADING, FONT_SIZE_HERO, FONT_SIZE_SMALL, FONT_SIZE_TITLE, GAP, ICON_GAP, ICON_SIZE_SMALL,
    LINE_HEIGHT_HERO, METER_HEIGHT, PAD, PANEL_PAD, PANEL_WIDTH, PILL_BUTTON_INSET, PILL_BUTTON_SIZE, RADIUS_PANEL,
    RADIUS_ROW, ROW_HEIGHT, ROW_INSET, SECTION_GAP, SHADOW_ROOM, WEIGHT_LABEL, WEIGHT_TITLE,
};
use nova_look::{Glass, Glyph, Icon, IconAssets, LookStyled, Shape, ShellWindow, Theme, Touching, region};
use nova_theme_info::{Palette, ThemeMode};
use serde::Deserialize;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const HELP: &str = "sample [switches]
  --theme FILE     a theme file (themes/<id>.toml); default: Carbonfox, compiled in
  --light          start in the theme's light colors
  --flip-after MS  swap dark and light once, MS after it opens
  --tint OPACITY   the glass tint's opacity, in place of the look's own";

/// The sample's own sizes: the pill's width, the panel's height, the dots
/// that show the theme's colors, the footer, and how full the meter is.
const PILL_WIDTH: Pixels = px(236.0);
const PANEL_HEIGHT: Pixels = px(404.0);
const SWATCH_SIZE: Pixels = px(14.0);
const FOOTER_HEIGHT: Pixels = px(40.0);
const METER_VALUE: f32 = 0.62;
const FOOTER_TINT: f32 = 0.9;
const PERCENT: f32 = 100.0;

struct Options {
    theme_file: Option<String>,
    light: bool,
    flip_after: Option<Duration>,
    tint: Option<f32>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options { theme_file: None, light: false, flip_after: None, tint: None };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--theme" => options.theme_file = Some(value()?),
            "--light" => options.light = true,
            "--flip-after" => {
                let millis = value()?.parse().map_err(|error| format!("--flip-after: {error}"))?;
                options.flip_after = Some(Duration::from_millis(millis));
            }
            "--tint" => options.tint = Some(value()?.parse().map_err(|error| format!("--tint: {error}"))?),
            "-h" | "--help" => return Err(HELP.into()),
            other => return Err(format!("unknown switch {other}\n{HELP}")),
        }
    }
    Ok(options)
}

/// A theme file (`nova-theme` owns the format).
#[derive(Deserialize)]
struct ThemeFile {
    name: String,
    dark: Palette,
    light: Option<Palette>,
}

/// The file's dark and light themes; light is dark again where the file has
/// no light colors.
fn read_themes(path: &str) -> Result<[Theme; 2], String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("can't read {path}: {error}"))?;
    let file: ThemeFile = toml::from_str(&text).map_err(|error| format!("{path}: {error}"))?;
    let id = std::path::Path::new(path).file_stem().and_then(|stem| stem.to_str()).unwrap_or(path);
    let theme = |mode, colors: &Palette| {
        Theme::from(&nova_theme_info::Theme { id: id.into(), name: file.name.clone(), mode, colors: colors.clone() })
    };
    let dark = theme(ThemeMode::Dark, &file.dark);
    let light = file.light.as_ref().map_or_else(|| dark.clone(), |colors| theme(ThemeMode::Light, colors));
    Ok([dark, light])
}

struct Sample {
    /// The dark and the light theme; a click swaps them.
    themes: [Theme; 2],
    tint: Option<f32>,
    width: Pixels,
    /// How full the meter is, on its way to [`METER_VALUE`].
    fill: Eased,
}

impl Sample {
    fn flip(&mut self, cx: &mut Context<Self>) {
        let [dark, light] = &self.themes;
        let next = if Theme::of(cx).mode == ThemeMode::Dark { light } else { dark };
        Theme::set(next.clone(), cx);
    }

    fn shapes(&self) -> [Shape; 2] {
        let centred = |width: Pixels, top: Pixels, height: Pixels| Bounds {
            origin: point((self.width - width) / 2.0, top),
            size: size(width, height),
        };
        [
            Shape::new(centred(PILL_WIDTH, BAR_GAP, BAR_HEIGHT), BAR_HEIGHT / 2.0),
            Shape::new(centred(PANEL_WIDTH, BAR_GAP + BAR_HEIGHT + GAP, PANEL_HEIGHT), RADIUS_PANEL),
        ]
    }

    fn glass(&self, shape: Shape, cx: &App) -> Div {
        let glass = Glass::new(shape.radius);
        self.tint
            .map_or(glass, |opacity| glass.tint_opacity(opacity))
            .build(cx)
            .absolute()
            .left(shape.bounds.left())
            .top(shape.bounds.top())
            .w(shape.bounds.size.width)
            .h(shape.bounds.size.height)
    }

    fn pill(&self, shape: Shape, cx: &App) -> Div {
        let theme = Theme::of(cx);
        self.glass(shape, cx)
            .flex()
            .items_center()
            .gap(GAP)
            .pl(PILL_BUTTON_INSET)
            .pr(PAD)
            .child(
                div()
                    .size(PILL_BUTTON_SIZE)
                    .rounded(PILL_BUTTON_SIZE / 2.0)
                    .bg(theme.hover())
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Icon::new(Glyph::SEARCH).size(ICON_SIZE_SMALL)),
            )
            .child(div().text_color(theme.fg_dim()).child("Sun 22"))
            .child(div().w(DIVIDER_WIDTH).h(DIVIDER_HEIGHT).bg(theme.outline()))
            .child(div().font_family(FONT_MONO).font_weight(WEIGHT_LABEL).child("21:54"))
            .child(div().flex_1())
            .child(Icon::new(Glyph::WIFI).filled(Glyph::WIFI_DOT).size(ICON_SIZE_SMALL))
            .child(Icon::new(Glyph::BELL).size(ICON_SIZE_SMALL).color(theme.fg_dim()))
    }

    fn panel(&self, shape: Shape, fill: f32, cx: &mut Context<Self>) -> Div {
        let theme = Theme::of(cx).clone();
        let swatches = div().flex().items_center().gap(ICON_GAP).children(
            theme
                .swatches
                .into_iter()
                .chain([theme.info, theme.success, theme.warning, theme.error, theme.secondary])
                .map(|color| div().size(SWATCH_SIZE).rounded(SWATCH_SIZE / 2.0).bg(color)),
        );
        self.glass(shape, cx)
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, cx.listener(|sample, _, _, cx| sample.flip(cx)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(SECTION_GAP)
                    .p(PANEL_PAD)
                    .child(header(&theme))
                    .child(headline(fill, &theme))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(row(Glyph::WIFI, "Wi-Fi", "Home", RowState::Selected, &theme))
                            .child(row(Glyph::BLUETOOTH, "Bluetooth", "2 devices", RowState::Hovered, &theme))
                            .child(row(Glyph::HEADPHONES, "Sound", "Headphones", RowState::Plain, &theme))
                            .child(row(Glyph::MIC_OFF, "Microphone", "Off", RowState::Disabled, &theme)),
                    )
                    .child(swatches),
            )
            .child(div().flex_1())
            // A fill that reaches the panel's bottom corners: without
            // fit_corners its square corners would stick out of the glass.
            .child(
                div()
                    .h(FOOTER_HEIGHT)
                    .fit_corners(shape.radius, px(0.0), Touching::Bottom)
                    .bg(theme.primary.alpha(FOOTER_TINT))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(ICON_GAP)
                    .text_color(theme.on_accent)
                    .font_weight(WEIGHT_TITLE)
                    .child(Icon::new(Glyph::CHECK).color(theme.on_accent))
                    .child("Apply"),
            )
    }
}

/// The panel's title, and the theme in use in a chip.
fn header(theme: &Theme) -> Div {
    let mode = if theme.mode == ThemeMode::Dark { "dark" } else { "light" };
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(div().text_size(FONT_SIZE_HEADING).font_weight(WEIGHT_TITLE).child("Look kit"))
        .child(
            div()
                .h(CHIP_HEIGHT)
                .px(CHIP_PAD)
                .rounded(CHIP_HEIGHT / 2.0)
                .bg(theme.surface_raised())
                .flex()
                .items_center()
                .text_size(FONT_SIZE_SMALL)
                .text_color(theme.fg_soft())
                .child(format!("{} {mode}", theme.id)),
        )
}

/// A headline number over a meter, both at `fill` (0..1).
fn headline(fill: f32, theme: &Theme) -> Div {
    let number = div()
        .flex()
        .items_end()
        .gap(ICON_GAP)
        .child(
            div()
                .text_size(FONT_SIZE_HERO)
                .line_height(LINE_HEIGHT_HERO)
                .font_weight(WEIGHT_TITLE)
                .child(format!("{:.0}", fill * PERCENT)),
        )
        .child(div().pb(GAP).text_size(FONT_SIZE_TITLE).text_color(theme.fg_dim()).child("%"));
    let meter = div()
        .h(METER_HEIGHT)
        .rounded(METER_HEIGHT / 2.0)
        .bg(theme.track())
        .child(div().h_full().w(relative(fill)).rounded(METER_HEIGHT / 2.0).bg(theme.primary));
    div().flex().flex_col().gap(GAP).child(number).child(meter)
}

enum RowState {
    Plain,
    Hovered,
    Selected,
    Disabled,
}

/// A list row: an icon, a label and a quieter word at its end.
fn row(glyph: Glyph, label: &'static str, detail: &'static str, state: RowState, theme: &Theme) -> Div {
    let ink = match state {
        RowState::Disabled => theme.fg_disabled(),
        _ => theme.fg,
    };
    let fill = match state {
        RowState::Selected => theme.selected(),
        RowState::Hovered => theme.hover(),
        RowState::Plain | RowState::Disabled => theme.fg.alpha(0.0),
    };
    div()
        .h(ROW_HEIGHT)
        .px(ROW_INSET)
        .rounded(RADIUS_ROW)
        .bg(fill)
        .flex()
        .items_center()
        .gap(GAP)
        .text_color(ink)
        .child(Icon::new(glyph).color(ink))
        .child(div().flex_1().child(label))
        .child(div().text_size(FONT_SIZE_SMALL).text_color(theme.fg_dim()).child(detail))
        .child(Icon::new(Glyph::RIGHT).size(ICON_SIZE_SMALL).color(theme.fg_dim()))
}

impl Render for Sample {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let now = Instant::now();
        // Frames only while the meter moves.
        if !self.fill.settled(now) {
            window.request_animation_frame();
        }
        let [pill, panel] = self.shapes();
        nova_look::set_blur_region(window, &[pill, panel]);
        window.set_input_region(Some(&region(&[pill, panel])));

        div().size_full().body_text(cx).child(self.pill(pill, cx)).child(self.panel(panel, self.fill.value(now), cx))
    }
}

fn open(options: Options, themes: Option<[Theme; 2]>, cx: &mut App) {
    let Some(display) = cx.displays().into_iter().next() else {
        eprintln!("sample: no screen to open on");
        cx.quit();
        return;
    };
    let themes = themes.unwrap_or_else(|| {
        let fallback = Theme::of(cx).clone();
        [fallback.clone(), fallback]
    });
    let [dark, light] = &themes;
    Theme::set(if options.light { light.clone() } else { dark.clone() }, cx);

    let width = display.bounds().size.width;
    let height = BAR_GAP + BAR_HEIGHT + GAP + PANEL_HEIGHT + SHADOW_ROOM;
    let opened = cx.open_window(ShellWindow::new(display.id(), size(width, height)).options(), move |_, cx| {
        cx.new(|cx| {
            if let Some(after) = options.flip_after {
                cx.spawn(async move |sample, cx| {
                    cx.background_executor().timer(after).await;
                    // The window may be gone by now; then there is nothing to flip.
                    sample.update(cx, |sample: &mut Sample, cx| sample.flip(cx)).ok();
                })
                .detach();
            }
            let now = Instant::now();
            let mut fill = Eased::lasting(0.0, DURATION_FILL, now);
            fill.set_target(METER_VALUE, now);
            Sample { themes, tint: options.tint, width, fill }
        })
    });
    if let Err(error) = opened {
        eprintln!("sample: could not open its window: {error:#}");
        cx.quit();
    }
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };
    let themes = match options.theme_file.as_deref().map(read_themes).transpose() {
        Ok(themes) => themes,
        Err(message) => {
            eprintln!("sample: {message}");
            return ExitCode::FAILURE;
        }
    };
    application().with_assets(IconAssets).run(move |cx: &mut App| open(options, themes, cx));
    ExitCode::SUCCESS
}
