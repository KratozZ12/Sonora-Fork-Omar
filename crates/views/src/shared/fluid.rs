use std::f32::consts::TAU;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{BoxShadow, Context, Hsla, Pixels, Render, Task, Window, div, point, px, relative};
use state::Sonora;
use ui::ActiveTheme as _;

// A slow liquid behind a page, made of the colours of one picture. A layer blur never
// reaches an image, so the photo itself is not used: its palette is painted as a few
// large blobs drifting on Lissajous paths, each one only a soft shadow so they melt
// into each other. Then it is veiled, because the page carries the contrast.
//
// Shadows and not a layer blur over the blobs: the shadow is blurred as it is drawn,
// while a layer blur depends on where the layer sits. Behind the lyrics, among the
// verses' own layers, it came out unblurred and on top of the veil.
const BLUR: Pixels = px(70.);
// The veil thickens down the page: the colour lives behind the header and the
// list below it stays easy to read.
const VEIL: (f32, f32) = (0.2, 0.8);
const FRAME: Duration = Duration::from_millis(33);
const ARRIVAL: f32 = 1.2;

struct Blob {
    size: f32,
    centre: (f32, f32),
    reach: (f32, f32),
    speed: (f32, f32),
    phase: (f32, f32),
    shade: f32,
}

const BLOBS: [Blob; 5] = [
    Blob {
        size: 0.9,
        centre: (0.2, 0.25),
        reach: (0.25, 0.2),
        speed: (0.07, 0.05),
        phase: (0.0, 1.3),
        shade: 0.0,
    },
    Blob {
        size: 0.8,
        centre: (0.8, 0.3),
        reach: (0.2, 0.25),
        speed: (0.05, 0.08),
        phase: (2.1, 0.4),
        shade: 0.0,
    },
    Blob {
        size: 0.75,
        centre: (0.5, 0.75),
        reach: (0.3, 0.2),
        speed: (0.06, 0.04),
        phase: (4.0, 2.7),
        shade: 0.0,
    },
    Blob {
        size: 0.6,
        centre: (0.3, 0.8),
        reach: (0.2, 0.15),
        speed: (0.09, 0.06),
        phase: (1.2, 5.1),
        shade: -0.12,
    },
    Blob {
        size: 0.55,
        centre: (0.7, 0.6),
        reach: (0.25, 0.3),
        speed: (0.04, 0.09),
        phase: (3.3, 3.9),
        shade: 0.12,
    },
];

pub(crate) struct Fluid {
    of: Option<String>,
    veil: (f32, f32),
    colours: Vec<Hsla>,
    arrived: Instant,
    reading: Option<Task<()>>,
    ticking: Option<Task<()>>,
    born: Instant,
}

impl Fluid {
    pub(crate) fn new() -> Self {
        Self {
            of: None,
            veil: VEIL,
            colours: Vec::new(),
            arrived: Instant::now(),
            reading: None,
            ticking: None,
            born: Instant::now(),
        }
    }

    /// How thickly the page's background lies over the liquid, at its top and bottom.
    pub(crate) fn veiled(mut self, veil: (f32, f32)) -> Self {
        self.veil = veil;
        self
    }

    pub(crate) fn paint(&mut self, picture: Option<String>, cx: &mut Context<Self>) {
        if picture == self.of {
            return;
        }
        self.of = picture.clone();
        self.colours.clear();
        self.reading = picture.map(|picture| {
            let found = ui::palette(picture, cx);
            cx.spawn(async move |this, cx| {
                let colours = found.await;
                this.update(cx, |this, cx| {
                    this.colours = colours;
                    this.arrived = Instant::now();
                    cx.notify();
                })
                .ok();
            })
        });
        cx.notify();
    }

    fn tick(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.ticking.is_some() {
            return;
        }
        let saved = match window.is_window_active() {
            true => None,
            false => Sonora::global(cx).settings.read(cx).saver().interval(),
        };
        let wait = FRAME.max(saved.unwrap_or_default());
        self.ticking = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                this.ticking = None;
                cx.notify();
            })
            .ok();
        }));
    }
}

impl Render for Fluid {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let backdrop = div().absolute().inset_0();
        if self.colours.is_empty() {
            return backdrop;
        }
        self.tick(window, cx);

        let background = cx.theme().background;
        let time = self.born.elapsed().as_secs_f32();
        let shown = (self.arrived.elapsed().as_secs_f32() / ARRIVAL).min(1.);
        let blobs = BLOBS.iter().enumerate().map(|(place, blob)| {
            let colour = self.colours[place % self.colours.len()];
            let colour = Hsla {
                s: (colour.s * 1.25).min(1.),
                l: (colour.l + blob.shade).clamp(0.08, 0.62),
                ..colour
            };
            let x = blob.centre.0 + blob.reach.0 * (time * blob.speed.0 * TAU + blob.phase.0).sin();
            let y = blob.centre.1 + blob.reach.1 * (time * blob.speed.1 * TAU + blob.phase.1).cos();
            let breath = 1. + 0.12 * (time * blob.speed.0 * 4. + blob.phase.1).sin();
            let size = blob.size * breath;

            div()
                .absolute()
                .left(relative(x - size / 2.))
                .top(relative(y - size / 2.))
                .w(relative(size))
                .h(relative(size))
                .rounded_full()
                .shadow(vec![BoxShadow {
                    color: colour,
                    offset: point(px(0.), px(0.)),
                    blur_radius: BLUR,
                    spread_radius: px(0.),
                    inset: false,
                }])
        });

        backdrop
            .overflow_hidden()
            .opacity(shown)
            .children(blobs)
            .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(background.opacity(self.veil.0), 0.),
                gpui::linear_color_stop(background.opacity(self.veil.1), 0.7),
            )))
    }
}
