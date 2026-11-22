//! Step 4: Nova's liquid volume capsule drawn by the fork's custom shader
//! patch (fork-patch/custom-shader.patch). Built only with `--features
//! shader` against the patched fork (scripts/build-shader.sh). The capsule
//! sits at the right edge of every screen; with `--toggles N` its level
//! changes N times, `--every` ms apart, and it quits after.

use std::time::{Duration, Instant};

use gpui::{
    App, Bounds, Context, IntoElement, Render, Size, Window, WindowBackgroundAppearance,
    WindowBounds, WindowKind, WindowOptions, canvas, div, layer_shell::*, point, prelude::*, px,
};

use crate::{FrameStats, Options};

const LIQUID: &str = include_str!("liquid.wgsl");

// Tokens.qml
const CAPSULE_WIDTH: f32 = 48.0;
const CAPSULE_HEIGHT: f32 = 260.0;
const CAPSULE_WALL: f32 = 4.0;
const ROOM: f32 = 40.0; // shadowBlur + shadowOffset
const SPRING_LEVEL: f32 = 70.0;
const SPRING_LEVEL_DAMPING: f32 = 0.85;
const SLOSH_MAX: f32 = 6.0;
const SLOSH_DECAY: f32 = 0.45;
const SLOSH_GAIN: f32 = 32.0;
const FIZZ: f32 = 0.3;
const FIZZ_PACE: f32 = 2.0;

fn rgba(rgb: u32, a: f32) -> [f32; 4] {
    [
        ((rgb >> 16) & 0xff) as f32 / 255.0,
        ((rgb >> 8) & 0xff) as f32 / 255.0,
        (rgb & 0xff) as f32 / 255.0,
        a,
    ]
}

struct Capsule {
    screen: String,
    refresh: Duration,
    stats: FrameStats,
    level: f32,
    level_target: f32,
    level_speed: f32,
    slosh: f32,
    time: f32,
    rise: f32,
    last: Instant,
    last_report: Instant,
}

impl Capsule {
    fn advance(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(1.0 / 30.0);
        self.last = now;
        self.time += dt;
        let c = 2.0 * SPRING_LEVEL_DAMPING * SPRING_LEVEL.sqrt();
        let a = -SPRING_LEVEL * (self.level - self.level_target) - c * self.level_speed;
        self.level_speed += a * dt;
        self.level = (self.level + self.level_speed * dt).clamp(0.0, 1.0);
        self.slosh = (self.slosh * (-dt / SLOSH_DECAY).exp()
            + self.level_speed.abs() * SLOSH_GAIN * dt)
            .min(SLOSH_MAX);
        self.rise += dt * (1.0 + FIZZ_PACE * FIZZ);
    }

    fn params(&self, scale: f32) -> [f32; gpui::SHADER_PARAMS] {
        let fg = 0xFFF6EB;
        let mut p = [0.0f32; gpui::SHADER_PARAMS];
        let rows: [[f32; 4]; 14] = [
            [ROOM * scale, ROOM * 0.75 * scale, CAPSULE_WIDTH * scale, CAPSULE_HEIGHT * scale],
            [CAPSULE_WIDTH / 2.0 * scale, CAPSULE_WALL * scale, self.level, self.time],
            [self.rise, 1.8 * scale, self.slosh * scale, 0.0],
            [1.6 * scale, 5.0 * scale, FIZZ, 14.0],
            [0.8 * scale, 1.5 * scale, 0.7, 0.85],
            [0.28, 6.0 * scale, 0.16, 18.0 * scale],
            [0.18, 0.07, 1.0, 1.2 * scale],
            [0.35, 0.45, 30.0 * scale, 10.0 * scale],
            rgba(0x0A0807, 0.55),
            rgba(fg, 0.12),
            rgba(fg, 0.60),
            rgba(0xBE95FF, 0.95),
            rgba(0x000000, 0.35),
            rgba(0xFFFFFF, 1.0),
        ];
        for (i, row) in rows.iter().enumerate() {
            p[i * 4..i * 4 + 4].copy_from_slice(row);
        }
        p
    }
}

impl Render for Capsule {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.stats.frame(self.refresh);
        self.advance();
        if self.last_report.elapsed() > Duration::from_secs(5) {
            self.stats.settled(&self.screen, self.refresh);
            self.last_report = Instant::now();
        }
        // It moves all the time while shown, like Nova's (the swell).
        window.request_animation_frame();
        let params = self.params(window.scale_factor());
        div().size_full().child(
            canvas(
                |_, _, _| {},
                move |bounds: Bounds<gpui::Pixels>, _, window, _| {
                    window.paint_shader(bounds, LIQUID, params);
                },
            )
            .size_full(),
        )
    }
}

pub fn open(cx: &mut App, options: &Options) {
    let width = CAPSULE_WIDTH + 2.0 * ROOM;
    let height = CAPSULE_HEIGHT + 2.0 * ROOM;
    for (index, display) in cx.displays().iter().enumerate() {
        if options.only_screen.is_some_and(|only| only != index) {
            continue;
        }
        let refresh = display
            .refresh_interval()
            .unwrap_or(Duration::from_secs_f64(1.0 / 60.0));
        let screen = format!("{index}:capsule");
        let toggles = options.toggles;
        let every = options.every;
        cx.open_window(
            WindowOptions {
                titlebar: None,
                display_id: Some(display.id()),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(0.0), px(0.0)),
                    size: Size::new(px(width), px(height)),
                })),
                app_id: Some("nova-proto".into()),
                inactive_frame_interval: None,
                window_background: WindowBackgroundAppearance::Transparent,
                kind: WindowKind::LayerShell(LayerShellOptions {
                    namespace: "nova-proto-capsule".into(),
                    layer: options.layer,
                    anchor: Anchor::RIGHT,
                    keyboard_interactivity: KeyboardInteractivity::None,
                    ..Default::default()
                }),
                ..Default::default()
            },
            move |_, cx| {
                cx.new(|cx| {
                    if toggles > 0 {
                        cx.spawn(async move |this, cx| {
                            for i in 0..toggles {
                                cx.background_executor().timer(every).await;
                                let target = if i % 2 == 0 { 0.8 } else { 0.3 };
                                if this
                                    .update(cx, |this: &mut Capsule, _| this.level_target = target)
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                            cx.update(|cx| cx.quit());
                        })
                        .detach();
                    }
                    Capsule {
                        screen,
                        refresh,
                        stats: FrameStats::default(),
                        level: 0.55,
                        level_target: 0.55,
                        level_speed: 0.0,
                        slosh: 0.0,
                        time: 0.0,
                        rise: 0.0,
                        last: Instant::now(),
                        last_report: Instant::now(),
                    }
                })
            },
        )
        .expect("open the capsule window");
    }
}
