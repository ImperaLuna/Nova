//! IMP-369 GPUI prototype (throwaway). One see-through stage window across
//! the top of every screen, holding Nova's central pill. Clicking the pill
//! morphs it into a launcher-sized panel and back. The click region follows
//! the drawn shape every frame, so clicks anywhere else reach the windows
//! below.
//!
//! Run `nova-proto --help` for the switches. Every motion that settles
//! prints one `frames` line per screen: frames drawn while it moved, how many
//! came later than 1.5 refresh intervals after the one before, and the worst
//! gap.

use std::cell::Cell;
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui::{
    App, Bounds, BoxShadow, Context, Hsla, IntoElement, MouseButton, Pixels, Render, Rgba, Size,
    Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions, div,
    layer_shell::*, point, prelude::*, px, rgba,
};
use gpui_platform::application;

// ---- Nova's tokens (ui/theme/Tokens.qml, themes/carbonfox.toml) ----------

const BAR_HEIGHT: f32 = 36.0;
const BAR_MARGIN: f32 = 8.0;
const RADIUS_PANEL: f32 = 24.0;
const PAD: f32 = 12.0;
const GAP: f32 = 8.0;
const WORKSPACE_SIZE: f32 = 26.0;
const WORKSPACE_GAP: f32 = 2.0;
const PILL_BUTTON_SIZE: f32 = 28.0;
const LAUNCHER_WIDTH: f32 = 620.0;
const LAUNCHER_HEIGHT: f32 = 440.0;
const LAUNCHER_PAD: f32 = 14.0;
const LAUNCHER_SEARCH_HEIGHT: f32 = 44.0;
const LAUNCHER_ROW_HEIGHT: f32 = 46.0;
const LAUNCHER_ROW_GAP: f32 = 2.0;
const LAUNCHER_GAP: f32 = 10.0;
const RADIUS_CONTROL: f32 = 12.0;
const RADIUS_ROW: f32 = 14.0;
const APP_ICON: f32 = 30.0;
const APP_ICON_RADIUS: f32 = 11.0;
const FONT_SIZE: f32 = 13.0;
const FONT_SIZE_SMALL: f32 = 12.0;
const FONT_SIZE_HEADING: f32 = 15.0;
const SHADOW_BLUR: f32 = 30.0;
const SHADOW_OFFSET: f32 = 10.0;
const DURATION_PANEL: Duration = Duration::from_millis(250);
const EASING: (f32, f32, f32, f32) = (0.2, 0.8, 0.2, 1.0);
// The stage is only as tall as the tallest panel, plus room for its shadow.
const STAGE_HEIGHT: f32 = BAR_MARGIN + LAUNCHER_HEIGHT + SHADOW_BLUR + SHADOW_OFFSET;
// Pill content fades out over the first share of the way, panel content in
// over the last (MorphReveal).
const PILL_FADE_OUT: f32 = 0.3;
const PANEL_FADE_IN_FROM: f32 = 0.55;

const FG: u32 = 0xFFF6EB;
const GLASS_TINT: u32 = 0x0A0807;
const PRIMARY: u32 = 0xFD7014;
const ON_ACCENT: u32 = 0x1D1D1D;
const GLASS_OPACITY: f32 = 0.55;
const FG_DIM_OPACITY: f32 = 0.66;

fn color(rgb: u32, alpha: f32) -> Hsla {
    let a = (alpha.clamp(0.0, 1.0) * 255.0).round() as u32;
    Rgba::from(rgba((rgb << 8) | a)).into()
}

// ---- Command line ---------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Motion {
    /// Nova's panel curve: 250 ms on cubic-bezier(0.2, 0.8, 0.2, 1).
    Curve,
    /// A spring (stiffness, damping ratio), so a toggle mid-way keeps speed.
    Spring,
}

#[derive(Clone)]
struct Options {
    motion: Motion,
    blur: bool,
    layer: Layer,
    toggles: u32,
    every: Duration,
    stiffness: f32,
    damping: f32,
    font: String,
    only_screen: Option<usize>,
    start_open: bool,
}

const HELP: &str = "nova-proto [switches]
  --motion curve|spring   panel motion (default curve, Nova's 250 ms curve)
  --spring K,Z            spring stiffness and damping ratio (default 260,0.82)
  --blur                  ask the compositor for blur (ext-background-effect)
  --layer top|overlay     layer-shell layer (default overlay, above the live bar)
  --toggles N             open/close N times on every screen by itself, then quit
  --every MS              time between automatic toggles (default 700)
  --screen I              only the I-th screen (0-based)
  --font NAME             UI font (default: fontconfig's sans-serif)
  --open                  start with the panel open";

fn parse_options() -> Options {
    let mut options = Options {
        motion: Motion::Curve,
        blur: false,
        layer: Layer::Overlay,
        toggles: 0,
        every: Duration::from_millis(700),
        stiffness: 260.0,
        damping: 0.82,
        font: String::new(),
        only_screen: None,
        start_open: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| panic!("{arg} needs a value"));
        match arg.as_str() {
            "--motion" => {
                options.motion = match value().as_str() {
                    "spring" => Motion::Spring,
                    _ => Motion::Curve,
                }
            }
            "--spring" => {
                let v = value();
                let (k, z) = v.split_once(',').expect("--spring K,Z");
                options.stiffness = k.parse().expect("stiffness");
                options.damping = z.parse().expect("damping ratio");
                options.motion = Motion::Spring;
            }
            "--blur" => options.blur = true,
            "--layer" => {
                options.layer = match value().as_str() {
                    "top" => Layer::Top,
                    _ => Layer::Overlay,
                }
            }
            "--toggles" => options.toggles = value().parse().expect("--toggles N"),
            "--every" => options.every = Duration::from_millis(value().parse().expect("ms")),
            "--screen" => options.only_screen = Some(value().parse().expect("index")),
            "--font" => options.font = value(),
            "--open" => options.start_open = true,
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            other => panic!("unknown switch {other}\n{HELP}"),
        }
    }
    if options.font.is_empty() {
        options.font = Command::new("fc-match")
            .args(["-f", "%{family[0]}", "sans-serif"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "sans-serif".into());
    }
    options
}

// ---- Motion ---------------------------------------------------------------

fn cubic_bezier(x: f32, (x1, y1, x2, y2): (f32, f32, f32, f32)) -> f32 {
    let bez = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let slope = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let mut t = x;
    for _ in 0..8 {
        let d = slope(t, x1, x2);
        if d.abs() < 1e-5 {
            break;
        }
        t = (t - (bez(t, x1, x2) - x) / d).clamp(0.0, 1.0);
    }
    bez(t, y1, y2)
}

/// Where the morph is: 0 the pill, 1 the open panel.
struct Morph {
    open: bool,
    t: f32,
    velocity: f32,
    /// Curve mode: where t started and when.
    from: f32,
    started: Instant,
    last_step: Option<Instant>,
}

impl Morph {
    fn moving(&self) -> bool {
        self.last_step.is_some()
    }

    fn toggle(&mut self) {
        self.open = !self.open;
        self.from = self.t;
        self.started = Instant::now();
        self.last_step.get_or_insert(Instant::now());
    }

    /// Advances to now; returns whether it is still moving.
    fn step(&mut self, options: &Options) -> bool {
        let Some(last) = self.last_step else {
            return false;
        };
        let now = Instant::now();
        let target = if self.open { 1.0 } else { 0.0 };
        let done = match options.motion {
            Motion::Curve => {
                let x = (now - self.started).as_secs_f32() / DURATION_PANEL.as_secs_f32();
                let x = x.min(1.0);
                self.t = self.from + (target - self.from) * cubic_bezier(x, EASING);
                x >= 1.0
            }
            Motion::Spring => {
                let mut dt = (now - last).as_secs_f32().min(1.0 / 30.0);
                let k = options.stiffness;
                let c = 2.0 * options.damping * k.sqrt();
                // Sub-steps keep it stable at low frame rates.
                while dt > 0.0 {
                    let h = dt.min(1.0 / 240.0);
                    let a = -k * (self.t - target) - c * self.velocity;
                    self.velocity += a * h;
                    self.t += self.velocity * h;
                    dt -= h;
                }
                (self.t - target).abs() < 0.0005 && self.velocity.abs() < 0.005
            }
        };
        if done {
            self.t = target;
            self.velocity = 0.0;
            self.last_step = None;
        } else {
            self.last_step = Some(now);
        }
        !done
    }
}

// ---- Frame timing ---------------------------------------------------------

#[derive(Default)]
struct FrameStats {
    previous: Option<Instant>,
    frames: u32,
    late: u32,
    worst: Duration,
    total_frames: u64,
    total_late: u64,
    motions: u32,
}

impl FrameStats {
    fn frame(&mut self, refresh: Duration) {
        let now = Instant::now();
        if let Some(previous) = self.previous {
            let gap = now - previous;
            self.worst = self.worst.max(gap);
            if gap > refresh.mul_f32(1.5) {
                self.late += 1;
            }
        }
        self.frames += 1;
        self.previous = Some(now);
    }

    fn settled(&mut self, screen: &str, refresh: Duration) {
        self.total_frames += u64::from(self.frames);
        self.total_late += u64::from(self.late);
        self.motions += 1;
        eprintln!(
            "frames screen={screen} motion={} drawn={} late={} worst={:.1}ms refresh={:.2}ms total_drawn={} total_late={} ({:.2}%)",
            self.motions,
            self.frames,
            self.late,
            self.worst.as_secs_f64() * 1000.0,
            refresh.as_secs_f64() * 1000.0,
            self.total_frames,
            self.total_late,
            100.0 * self.total_late as f64 / self.total_frames.max(1) as f64,
        );
        self.previous = None;
        self.frames = 0;
        self.late = 0;
        self.worst = Duration::ZERO;
    }
}

// ---- Clock ----------------------------------------------------------------

/// The local offset from UTC, read once (no time-zone crate in a prototype).
fn utc_offset_secs() -> i64 {
    Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| {
            let s = s.trim();
            let sign = if s.starts_with('-') { -1 } else { 1 };
            let h: i64 = s.get(1..3)?.parse().ok()?;
            let m: i64 = s.get(3..5)?.parse().ok()?;
            Some(sign * (h * 3600 + m * 60))
        })
        .unwrap_or(0)
}

fn day_and_time(offset: i64) -> (String, String) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 + offset;
    let days = secs.div_euclid(86_400);
    let of_day = secs.rem_euclid(86_400);
    // 1970-01-01 was a Thursday.
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"][days.rem_euclid(7) as usize];
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    (
        format!("{weekday} {day}"),
        format!("{:02}:{:02}", of_day / 3600, (of_day / 60) % 60),
    )
}

// ---- The stage ------------------------------------------------------------

struct Stage {
    options: Options,
    screen: String,
    width: f32,
    morph: Morph,
    stats: FrameStats,
    refresh: Duration,
    offset: i64,
    region: Option<(Bounds<Pixels>, f32)>,
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn pill_width() -> f32 {
    // Search button, three workspace numbers, the day and the time.
    PAD / 2.0 + PILL_BUTTON_SIZE + GAP + 3.0 * WORKSPACE_SIZE + 2.0 * WORKSPACE_GAP + GAP * 2.0
        + 40.0
        + GAP
        + 34.0
        + PAD
}

/// A rounded rectangle as a stack of plain ones, for the click region.
fn rounded_region(b: Bounds<Pixels>, radius: f32) -> Vec<Bounds<Pixels>> {
    let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let r = radius.min(w / 2.0).min(h / 2.0);
    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds {
        origin: point(px(x), px(y)),
        size: Size::new(px(w), px(h)),
    };
    let mut out = vec![rect(x, y + r, w, h - 2.0 * r)];
    const SLICES: usize = 6;
    let slice = r / SLICES as f32;
    for i in 0..SLICES {
        // Inset of the arc at the slice's middle.
        let dy = r - (i as f32 + 0.5) * slice;
        let inset = r - (r * r - dy * dy).max(0.0).sqrt();
        let top = y + i as f32 * slice;
        let bottom = y + h - (i as f32 + 1.0) * slice;
        out.push(rect(x + inset, top, w - 2.0 * inset, slice));
        out.push(rect(x + inset, bottom, w - 2.0 * inset, slice));
    }
    out
}

impl Stage {
    fn toggle(&mut self, cx: &mut Context<Self>) {
        self.morph.toggle();
        cx.notify();
    }

    fn shape(&self) -> (Bounds<Pixels>, f32) {
        let t = self.morph.t;
        let pw = pill_width();
        let (w, h) = (lerp(pw, LAUNCHER_WIDTH, t), lerp(BAR_HEIGHT, LAUNCHER_HEIGHT, t));
        let x = (self.width - w) / 2.0;
        let radius = lerp(BAR_HEIGHT / 2.0, RADIUS_PANEL, t.clamp(0.0, 1.0));
        (
            Bounds {
                origin: point(px(x), px(BAR_MARGIN)),
                size: Size::new(px(w.max(1.0)), px(h.max(1.0))),
            },
            radius,
        )
    }

    fn pill_content(&self) -> impl IntoElement {
        let (day, time) = day_and_time(self.offset);
        let number = |n: u32, active: bool| {
            div()
                .w(px(WORKSPACE_SIZE))
                .h(px(WORKSPACE_SIZE))
                .rounded(px(WORKSPACE_SIZE / 2.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(FONT_SIZE_SMALL))
                .when(active, |d| d.bg(color(PRIMARY, 1.0)).text_color(color(ON_ACCENT, 1.0)))
                .when(!active, |d| d.text_color(color(FG, FG_DIM_OPACITY)))
                .child(n.to_string())
        };
        div()
            .w(px(pill_width()))
            .h(px(BAR_HEIGHT))
            .flex()
            .items_center()
            .pl(px(PAD / 2.0 - 2.0))
            .pr(px(PAD))
            .gap(px(GAP))
            .text_size(px(FONT_SIZE))
            .text_color(color(FG, 1.0))
            // Search button: a ring and its handle.
            .child(
                div()
                    .w(px(PILL_BUTTON_SIZE))
                    .h(px(PILL_BUTTON_SIZE))
                    .rounded(px(PILL_BUTTON_SIZE / 2.0))
                    .bg(color(FG, 0.06))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(11.0))
                            .h(px(11.0))
                            .rounded(px(6.0))
                            .border_2()
                            .border_color(color(FG, 0.9)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(WORKSPACE_GAP))
                    .child(number(1, false))
                    .child(number(2, true))
                    .child(number(3, false)),
            )
            .child(div().text_color(color(FG, FG_DIM_OPACITY)).child(day))
            .child(div().child(time))
    }

    fn panel_content(&self) -> impl IntoElement {
        let apps = [
            ("Firefox", "Web browser", 0x5988DB),
            ("Nova Files", "File manager", 0xE8650E),
            ("Ghostty", "Terminal", 0x07A7A5),
            ("Zed", "Code editor", 0x9067CC),
            ("Spotify", "Music", 0x029A43),
            ("Signal", "Messages", 0x049BE7),
        ];
        let row = |i: usize, (name, about, swatch): (&str, &str, u32)| {
            div()
                .h(px(LAUNCHER_ROW_HEIGHT))
                .rounded(px(RADIUS_ROW))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(12.0))
                .when(i == 0, |d| d.bg(color(PRIMARY, 0.16)))
                .child(
                    div()
                        .w(px(APP_ICON))
                        .h(px(APP_ICON))
                        .rounded(px(APP_ICON_RADIUS))
                        .bg(color(swatch, 1.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(color(FG, 1.0))
                        .child(name[..1].to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(div().text_size(px(FONT_SIZE)).child(name.to_string()))
                        .child(
                            div()
                                .text_size(px(FONT_SIZE_SMALL))
                                .text_color(color(FG, FG_DIM_OPACITY))
                                .child(about.to_string()),
                        ),
                )
        };
        div()
            .w(px(LAUNCHER_WIDTH))
            .h(px(LAUNCHER_HEIGHT))
            .flex()
            .flex_col()
            .text_color(color(FG, 1.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(LAUNCHER_GAP))
                    .p(px(LAUNCHER_PAD))
                    .child(
                        div()
                            .h(px(LAUNCHER_SEARCH_HEIGHT))
                            .rounded(px(RADIUS_CONTROL))
                            .bg(color(FG, 0.06))
                            .px(px(14.0))
                            .flex()
                            .items_center()
                            .text_size(px(FONT_SIZE_HEADING))
                            .text_color(color(FG, FG_DIM_OPACITY))
                            .child("Search apps"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(LAUNCHER_ROW_GAP))
                            .children(apps.into_iter().enumerate().map(|(i, a)| row(i, a))),
                    ),
            )
            // A full-width footer that touches the bottom corners: if
            // clipping were square, its corners would poke out of the panel.
            .child(div().flex_grow(1.0))
            .child(div().h(px(34.0)).bg(color(PRIMARY, 0.22)))
    }
}

impl Render for Stage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.morph.moving() {
            self.stats.frame(self.refresh);
            if self.morph.step(&self.options) {
                window.request_animation_frame();
            } else {
                self.stats.settled(&self.screen, self.refresh);
            }
        }

        let (shape, radius) = self.shape();
        if self.region != Some((shape, radius)) {
            window.set_input_region(Some(&rounded_region(shape, radius)));
            self.region = Some((shape, radius));
        }

        let t = self.morph.t;
        let pill_opacity = (1.0 - t / PILL_FADE_OUT).clamp(0.0, 1.0);
        let panel_opacity = ((t - PANEL_FADE_IN_FROM) / (1.0 - PANEL_FADE_IN_FROM)).clamp(0.0, 1.0);
        let w = f32::from(shape.size.width);

        div().size_full().font_family(self.options.font.clone()).child(
            div()
                .id("shape")
                .absolute()
                .left(shape.origin.x)
                .top(shape.origin.y)
                .w(shape.size.width)
                .h(shape.size.height)
                .rounded(px(radius))
                .bg(color(GLASS_TINT, GLASS_OPACITY))
                .border_1()
                .border_color(color(FG, 0.10))
                .shadow(vec![BoxShadow {
                    color: color(0x000000, 0.35),
                    offset: point(px(0.0), px(SHADOW_OFFSET)),
                    blur_radius: px(SHADOW_BLUR),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .overflow_hidden()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.toggle(cx)),
                )
                .when(pill_opacity > 0.0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(0.0))
                            .left(px((w - pill_width()) / 2.0))
                            .opacity(pill_opacity)
                            .child(self.pill_content()),
                    )
                })
                .when(panel_opacity > 0.0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(0.0))
                            .left(px((w - LAUNCHER_WIDTH) / 2.0))
                            .opacity(panel_opacity)
                            .child(self.panel_content()),
                    )
                }),
        )
    }
}

fn main() {
    let options = parse_options();
    let offset = utc_offset_secs();
    application().run(move |cx: &mut App| {
        let displays = cx.displays();
        let toggles_left = Rc::new(Cell::new(0));
        for (index, display) in displays.iter().enumerate() {
            if options.only_screen.is_some_and(|only| only != index) {
                continue;
            }
            let bounds = display.bounds();
            let width = f32::from(bounds.size.width);
            let refresh = display
                .refresh_interval()
                .unwrap_or(Duration::from_secs_f64(1.0 / 60.0));
            let screen = format!("{index}:{}x{}", width, f32::from(bounds.size.height));
            eprintln!(
                "screen {screen} id={:?} refresh={:.2}ms font={}",
                display.id(),
                refresh.as_secs_f64() * 1000.0,
                options.font
            );
            toggles_left.set(toggles_left.get() + options.toggles);
            let options = options.clone();
            let toggles_left = toggles_left.clone();
            cx.open_window(
                WindowOptions {
                    titlebar: None,
                    display_id: Some(display.id()),
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(0.0), px(0.0)),
                        size: Size::new(px(width), px(STAGE_HEIGHT)),
                    })),
                    app_id: Some("nova-proto".into()),
                    // A shell surface never has keyboard focus, so GPUI would
                    // treat it as an inactive window and cap it at 30 fps.
                    inactive_frame_interval: None,
                    window_background: if options.blur {
                        WindowBackgroundAppearance::Blurred
                    } else {
                        WindowBackgroundAppearance::Transparent
                    },
                    kind: WindowKind::LayerShell(LayerShellOptions {
                        namespace: "nova-proto".into(),
                        layer: options.layer,
                        anchor: Anchor::TOP | Anchor::LEFT | Anchor::RIGHT,
                        exclusive_zone: None,
                        keyboard_interactivity: KeyboardInteractivity::None,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |_window, cx| {
                    cx.new(|cx| {
                        // The clock changes once a minute; nothing else runs
                        // while idle.
                        cx.spawn(async move |this, cx| {
                            loop {
                                cx.background_executor().timer(Duration::from_secs(20)).await;
                                if this.update(cx, |_: &mut Stage, cx| cx.notify()).is_err() {
                                    break;
                                }
                            }
                        })
                        .detach();
                        if options.toggles > 0 {
                            let every = options.every;
                            let count = options.toggles;
                            let left = toggles_left.clone();
                            cx.spawn(async move |this, cx| {
                                // Let the first frame and the GPU settle.
                                cx.background_executor().timer(Duration::from_secs(2)).await;
                                for _ in 0..count {
                                    cx.background_executor().timer(every).await;
                                    if this.update(cx, |this: &mut Stage, cx| this.toggle(cx)).is_err() {
                                        return;
                                    }
                                    left.set(left.get() - 1);
                                }
                                // Wait for the last motion, then quit when
                                // every screen is done.
                                cx.background_executor().timer(Duration::from_secs(1)).await;
                                if left.get() == 0 {
                                    let _ = cx.update(|cx| cx.quit());
                                }
                            })
                            .detach();
                        }
                        let start_open = options.start_open;
                        Stage {
                            options,
                            screen,
                            width,
                            morph: Morph {
                                open: start_open,
                                t: if start_open { 1.0 } else { 0.0 },
                                velocity: 0.0,
                                from: 0.0,
                                started: Instant::now(),
                                last_step: None,
                            },
                            stats: FrameStats::default(),
                            refresh,
                            offset,
                            region: None,
                        }
                    })
                },
            )
            .expect("open the stage window");
        }
    });
}
