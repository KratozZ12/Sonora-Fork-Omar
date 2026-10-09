use std::{collections::HashMap, ops::Range};

use gpui::prelude::*;

use gpui::{
    Animation, AnimationExt as _, App, Bounds, Context, Div, DragMoveEvent, Entity, FontWeight,
    MouseDownEvent, Pixels, Point, Render, ScrollHandle, ScrollStrategy, ScrollWheelEvent,
    SharedString, SpringConfig, SpringState, Task, UniformListScrollHandle, Window, div,
    ease_in_out, px, svg, uniform_list,
};
use i18n::t;
use music::{Track, Voice};
use router::{Destination, LibraryTab, Link as _, LocalTab};
use state::{
    AppSettings, Lyrics, LyricsState, Playback, PlaybackState, Queue, RomanizationScripts, SideTab,
    Sonora, Whence,
};
use ui::Faced as _;
use ui::{
    ActiveTheme as _, Button, Card, DraggedPin, Edge, MenuItem, Motion, Picker, Pin, Pinnable as _,
    Popovers, Popup, Scrollbar, Scroller, Spot, Springs, Text, Vacancy, drop_gap, drop_marker,
    ease_out_cubic, ease_out_expo, eyebrow, faint, mix, snapped, vacant,
};

use crate::chrome::verse::{self, Look, Verse};
use crate::chrome::{Chrome, section_label};
use crate::shared::effects;
use crate::shared::fluid::Fluid;
use crate::shared::menus::ItemMenu;
use crate::shared::pins::Pinned as _;

const QUEUE: &str = "queue";
const BULLET: SharedString = SharedString::new_static("·");
const FADE: f32 = 96.;
const REST: f32 = FADE * 0.75;
const TAIL_ROWS: usize = 2;
const BLUR: f32 = 0.07;
const VEIL: f32 = 0.5;
// the verses either side stay sharp
const HAZE: f32 = 0.3;
const VERSE_FADE: f32 = 1.25;
const PAST: f32 = 0.4;
const AHEAD: f32 = 0.6;
const ACTIVE_VERSE_GROWTH: Pixels = px(2.);
const FULLSCREEN_VERSE_GROWTH: Pixels = px(3.);
// The cover behind the verses, as the liquid of its colours rather than the picture
// itself: a cover with a shape in it (a framed painting on a plain field) kept that
// shape through any blur cheap enough to run, and read as a box behind the words.
// Then it is veiled, because the words carry the contrast.
const AMBIENCE_VEIL: f32 = 0.74;
const AMBIENCE_EDGE: f32 = 0.45;
// The glow under a sung letter: the letters again, blurred, each as bright as it
// is lit. A held word pulses and shines harder.
const GLOW_BLUR: f32 = 0.3;
const GLOW: f32 = 0.6;
const GLOW_HELD: f32 = 1.;
const GLOW_RISE: f32 = 0.15;
const GLOW_FADE: f32 = 0.6;
const GLOW_HELD_FADE: f32 = 0.5;
const PULSE: f32 = 0.25;
const PULSE_BEAT: f32 = 0.14;
const PINNED_SHARE: f32 = 0.25;
const PIN: f32 = 0.3;
// how far a row falls behind, in verse sizes
const LAG: f32 = 24.;
// never past this share
const LAG_SHARE: f32 = 0.28;
// movement the last row skips
const LAG_TRAIL: f32 = 0.9;
// The first row's physical spring. Rows farther along the viewport keep the same damping ratio but
// use a lower natural frequency, producing the cascading iMessage-like settle.
const LAG_STAGGER: f32 = 0.35;
const LAG_LEAST: Pixels = px(0.05);
const LAG_STALL: f32 = 0.064;
// How far a blur reaches past what it blurs, in standard deviations: the renderer
// draws four. A blurred layer is cut square at its own bounds, so anything that
// glows needs this much room around it or its glow ends in a hard edge.
const BLUR_REACH: f32 = 4.;
// How far a verse sinks while it is held.
const PRESSED: f32 = 0.955;
// The widest a line of lyrics is set, in multiples of its own size. Left to fill
// a fullscreen panel, a lead verse and a background one end up at opposite edges.
const REACH: f32 = 24.;
// What a sheet settling on the best answer comes in through: it blurs and fades
// on the way, once, on a curve that is the same going in as coming out.
const RESOLVE_BLUR: f32 = 0.2;
const RESOLVE_FADE: f32 = 0.5;
const SETTLE: std::time::Duration = std::time::Duration::from_secs(4);
const INSTRUMENTAL_BREAK: std::time::Duration = std::time::Duration::from_secs(5);
// karaoke sweep ceiling
const KARAOKE_HZ: u32 = 45;
const KARAOKE_FRAME: std::time::Duration =
    std::time::Duration::from_nanos(1_000_000_000 / KARAOKE_HZ as u64);
// Letter motion, in verse sizes and seconds. A word held this long waves.
const HELD: std::time::Duration = std::time::Duration::from_millis(900);
const WAVE: f32 = 0.18;
const WAVE_TAIL: f32 = 0.45;
const WAVE_LEAST: f32 = 3.;
const WAVE_EASE: f32 = 0.35;
const NUDGE: f32 = 0.1;
const NUDGE_RISE: f32 = 0.22;
const NUDGE_SETTLE: f32 = 0.45;
// a backing lane, against its verse
const SOFT: f32 = 0.6;
const LANE_TOP: f32 = 0.85;

fn track(queue: &Queue, position: QueuePosition) -> Option<Track> {
    match position {
        QueuePosition::Past(index) => queue.past().nth(index).cloned(),
        QueuePosition::Current => queue.current().cloned(),
        QueuePosition::Upcoming(index) => queue.upcoming().nth(index).cloned(),
        QueuePosition::Similar(index) => queue.similar().nth(index).cloned(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum QueuePosition {
    Past(usize),
    Current,
    Upcoming(usize),
    Similar(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Slot {
    Header(&'static str),
    Track(QueuePosition),
}

#[derive(Clone, Copy)]
struct Sections {
    past: usize,
    current: bool,
    upcoming: usize,
    similar: usize,
}

impl Sections {
    fn past_end(self) -> usize {
        match self.past {
            0 => 0,
            count => count + 1,
        }
    }

    fn current_end(self) -> usize {
        self.past_end() + 2 * usize::from(self.current)
    }

    fn upcoming_end(self) -> usize {
        self.current_end()
            + match self.upcoming {
                0 => 0,
                count => count + 1,
            }
    }

    fn len(self) -> usize {
        self.upcoming_end()
            + match self.similar {
                0 => 0,
                count => count + 1,
            }
    }

    fn current_index(self) -> Option<usize> {
        self.current.then(|| self.past_end() + 1)
    }

    fn slot(self, index: usize) -> Slot {
        if index < self.past_end() {
            return match index {
                0 => Slot::Header("queue-history"),
                _ => Slot::Track(QueuePosition::Past(index - 1)),
            };
        }
        if index < self.current_end() {
            return match index == self.past_end() {
                true => Slot::Header("queue-now-playing"),
                false => Slot::Track(QueuePosition::Current),
            };
        }
        if index < self.upcoming_end() {
            return match index == self.current_end() {
                true => Slot::Header("queue-up-next"),
                false => Slot::Track(QueuePosition::Upcoming(index - self.current_end() - 1)),
            };
        }
        match index == self.upcoming_end() {
            true => Slot::Header("queue-similar"),
            false => Slot::Track(QueuePosition::Similar(index - self.upcoming_end() - 1)),
        }
    }
}

/// A place in the sheet the pointer can be over: a verse, or the melody break
/// above it. They share a line index, so they need telling apart.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Warm {
    Verse(usize),
    Break(usize),
}

/// How near the pointer a spot is, and how far it has been pressed.
#[derive(Clone, Copy, Default)]
struct Touch {
    warmth: f32,
    depth: f32,
    waking: bool,
    settling: bool,
}

#[derive(Clone, Copy)]
struct Sung {
    karaoke: bool,
    lane: Pixels,
    scripts: Option<RomanizationScripts>,
    theme: ui::Theme,
    karaoke_tint: gpui::Hsla,
    // letters lift as they are sung
    motion: bool,
}

#[derive(Clone, Copy)]
struct RowLook {
    playing: bool,
    drop_line: Option<Edge>,
}

#[derive(Clone)]
struct ContextMenuState {
    track: Track,
    revision: u64,
    position: Point<Pixels>,
}

impl QueuePosition {
    fn past(self) -> Option<usize> {
        match self {
            Self::Past(index) => Some(index),
            _ => None,
        }
    }

    fn upcoming(self) -> Option<usize> {
        match self {
            Self::Upcoming(index) => Some(index),
            _ => None,
        }
    }

    fn similar(self) -> Option<usize> {
        match self {
            Self::Similar(index) => Some(index),
            _ => None,
        }
    }
}

pub(crate) struct Aside {
    queue: Entity<Queue>,
    playback: Entity<Playback>,
    lyrics: Entity<Lyrics>,
    settings: Entity<AppSettings>,
    tab: SideTab,
    verse_bar: Entity<Scrollbar>,
    followed: Option<usize>,
    nudges: u64,
    pinned: bool,
    nudged: Option<std::time::Instant>,
    verse_of: Option<String>,
    verse_take: u64,
    placing: bool,
    context_menu: Option<ContextMenuState>,
    track_menu: ItemMenu,
    drop_gap: Option<usize>,
    scroll: UniformListScrollHandle,
    scrollbar: Entity<Scrollbar>,
    past_len: usize,
    anchor: bool,
    titled: bool,
    aiming: bool,
    rested: Option<Pixels>,
    since: std::time::Instant,
    over: Option<Warm>,
    hovered: Option<Warm>,
    fading: Option<Warm>,
    linger: Option<Task<()>>,
    previous_active_line: Option<usize>,
    departing_line: Option<usize>,
    departed: std::time::Instant,
    arrived: std::time::Instant,
    arrival: u64,
    departure: u64,
    seen: Pixels,
    flying: bool,
    flew: bool,
    slid: std::time::Instant,
    drifts: HashMap<usize, SpringState>,
    pinning: Option<usize>,
    held: Option<Warm>,
    rising: Option<Warm>,
    sank: std::time::Instant,
    sinking: Option<Task<()>>,
    swept_frame: std::time::Instant,
    sweeping: Option<Task<()>>,
    showed: bool,
    resolving: bool,
    ambience_of: Option<String>,
    ambience: Entity<Fluid>,
    sources: Popovers,
}

impl Aside {
    pub(crate) fn new(
        queue: Entity<Queue>,
        playback: Entity<Playback>,
        tab: SideTab,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&queue, |this, queue, cx| {
            let revision = queue.read(cx).revision();
            if this
                .context_menu
                .as_ref()
                .is_some_and(|menu| menu.revision != revision)
            {
                this.track_menu.reset(cx);
                this.context_menu = None;
            }
            cx.notify();
        })
        .detach();
        cx.observe(&playback, |_, _, cx| cx.notify()).detach();
        let chrome = Chrome::entity(cx);
        cx.observe(&chrome, |_, _, cx| cx.notify()).detach();

        let me = cx.entity_id();
        let scroll = UniformListScrollHandle::new();
        let scrollbar =
            cx.new(|_| Scrollbar::new(scroll.0.borrow().base_handle.clone()).watching(me));
        let playlist_scrollbar = cx.new(|_| Scrollbar::inset().watching(me));
        let lyrics = Sonora::global(cx).lyrics.clone();
        cx.observe(&lyrics, |_, _, cx| cx.notify()).detach();
        let settings = Sonora::global(cx).settings.clone();
        cx.observe(&settings, |_, _, cx| cx.notify()).detach();
        let verse_bar = cx.new(|_| {
            Scrollbar::new(ScrollHandle::new())
                .spring(Springs::LYRICS_SCROLL)
                .watching(me)
        });

        Self {
            queue,
            playback,
            lyrics,
            settings,
            tab,
            verse_bar,
            followed: None,
            nudges: 0,
            pinned: true,
            nudged: None,
            verse_of: None,
            verse_take: 0,
            placing: false,
            context_menu: None,
            track_menu: ItemMenu::new(playlist_scrollbar),
            drop_gap: None,
            scroll,
            scrollbar,
            past_len: 0,
            anchor: true,
            titled: true,
            aiming: false,
            rested: None,
            since: std::time::Instant::now(),
            over: None,
            hovered: None,
            fading: None,
            linger: None,
            previous_active_line: None,
            departing_line: None,
            departed: std::time::Instant::now(),
            arrived: std::time::Instant::now(),
            arrival: 0,
            departure: 0,
            seen: px(0.),
            flying: false,
            flew: true,
            slid: std::time::Instant::now(),
            drifts: HashMap::new(),
            pinning: None,
            held: None,
            rising: None,
            sank: std::time::Instant::now(),
            sinking: None,
            swept_frame: std::time::Instant::now(),
            sweeping: None,
            showed: false,
            resolving: false,
            ambience_of: None,
            ambience: cx.new(|_| Fluid::new().veiled((0., 0.))),
            sources: Popovers::default(),
        }
    }

    pub(crate) fn strip(&mut self) {
        self.titled = false;
    }

    pub(crate) fn tab(&self) -> SideTab {
        self.tab
    }

    pub(crate) fn show(&mut self, tab: SideTab, cx: &mut Context<Self>) {
        if self.tab != tab {
            self.tab = tab;
            self.forget_verse();
            self.anchor_verse();
        }
        self.anchor = true;
        cx.notify();
    }

    pub(crate) fn dismiss(&mut self, cx: &mut Context<Self>) {
        self.track_menu.reset(cx);
        self.context_menu = None;
        cx.notify();
    }

    /// How far a verse has sunk under the pointer, or risen back after being let
    /// go of.
    fn sink_progress(&self, window: &mut Window) -> f32 {
        let span = Motion::Quick.span().as_secs_f32().max(f32::EPSILON);
        let progress = (self.sank.elapsed().as_secs_f32() / span).clamp(0., 1.);
        if progress < 1. {
            window.request_animation_frame();
        }
        ease_in_out(progress)
    }

    fn touch(&self, spot: Warm, sharpen: f32, sink: f32) -> Touch {
        let waking = self.hovered == Some(spot);
        let settling = self.fading == Some(spot);
        Touch {
            warmth: match (waking, settling) {
                (true, _) => sharpen,
                (_, true) => 1. - sharpen,
                _ => 0.,
            },
            depth: match (self.held == Some(spot), self.rising == Some(spot)) {
                (true, _) => sink,
                (_, true) => 1. - sink,
                _ => 0.,
            },
            waking,
            settling,
        }
    }

    fn press_verse(&mut self, spot: Warm, down: bool, cx: &mut Context<Self>) {
        match down {
            true => {
                if self.held == Some(spot) {
                    return;
                }
                self.held = Some(spot);
                self.rising = None;
                self.sinking = None;
                self.sank = std::time::Instant::now();
            }
            false => {
                if self.held != Some(spot) {
                    return;
                }
                self.held = None;
                self.rising = Some(spot);
                self.sank = std::time::Instant::now();
                self.sinking = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Motion::Quick.span()).await;
                    this.update(cx, |this, cx| {
                        if this.rising != Some(spot) {
                            return;
                        }
                        this.rising = None;
                        cx.notify();
                    })
                    .ok();
                }));
            }
        }
        cx.notify();
    }

    fn sweep_karaoke(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.sweeping.is_some() {
            return;
        }
        let saved = match window.is_window_active() {
            true => None,
            false => self.settings.read(cx).saver().interval(),
        };
        let interval = KARAOKE_FRAME.max(saved.unwrap_or_default());
        let wait = interval.saturating_sub(self.swept_frame.elapsed());
        self.sweeping = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                this.swept_frame = std::time::Instant::now();
                this.sweeping = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn sharpen_progress(&self, window: &mut Window) -> f32 {
        let span = Motion::Quick.span().as_secs_f32().max(f32::EPSILON);
        let progress = (self.since.elapsed().as_secs_f32() / span).clamp(0., 1.);
        if progress < 1. {
            window.request_animation_frame();
        }
        ease_in_out(progress)
    }

    fn forget_verse(&mut self) {
        self.flying = false;
        self.pinning = None;
        self.previous_active_line = None;
        self.departing_line = None;
        self.placing = true;
        self.forget_measurements();
    }

    fn forget_measurements(&mut self) {
        self.drifts.clear();
    }

    // the panel took the wheel
    fn flown(&mut self, goal: Pixels, from: Pixels) {
        self.flying = true;
        self.flew = goal <= from;
    }

    // only automatic scrolls
    fn lagged(
        &mut self,
        scroll: &ScrollHandle,
        presentation: Pixels,
        verse: Pixels,
        nudges: u64,
    ) -> Drag {
        let now = std::time::Instant::now();
        let beat = now.duration_since(self.slid).as_secs_f32().min(LAG_STALL);
        self.slid = now;

        // follow the seen position
        let offset = scroll.offset().y + presentation;
        let step = offset - self.seen;
        self.seen = offset;
        if nudges != self.nudges {
            self.flying = false;
        }

        Drag {
            step: match self.flying {
                true => step,
                false => px(0.),
            },
            beat,
            downward: self.flew,
            most: (verse * LAG).min(scroll.bounds().size.height * LAG_SHARE),
        }
    }

    // A physical spring per row. Feeding the inverse scroll delta makes each row lag behind the
    // sheet; retaining velocity lets it settle naturally and survive a retarget without restarting.
    fn dragged(&mut self, row: usize, along: f32, drag: Drag, window: &mut Window) -> Pixels {
        let held = self.drifts.get(&row).copied();
        if held.is_none() && drag.step == px(0.) {
            return px(0.);
        }
        let mut state = held.unwrap_or_default();
        state.position = (px(state.position) - drag.step * (LAG_TRAIL * along))
            .clamp(-drag.most, drag.most)
            .as_f32();
        let spring = lag_spring(along);
        state = spring.step(state, 0., drag.beat);
        if spring.is_settled(state, 0., LAG_LEAST.as_f32()) {
            self.drifts.remove(&row);
            return px(0.);
        }
        self.drifts.insert(row, state);
        window.request_animation_frame();
        px(state.position)
    }

    fn set_hovered(&mut self, spot: Warm, over: bool, cx: &mut Context<Self>) {
        if !over {
            if self.over == Some(spot) {
                self.over = None;
            }
            if self.hovered == Some(spot) {
                self.hovered = None;
                self.fading = Some(spot);
                self.since = std::time::Instant::now();
                self.linger = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(Motion::Quick.span()).await;
                    this.update(cx, |this, cx| {
                        if this.fading != Some(spot) {
                            return;
                        }
                        this.fading = None;
                        cx.notify();
                    })
                    .ok();
                }));
                cx.notify();
            }
            return;
        }

        self.over = Some(spot);
        if self.hovered == Some(spot) {
            return;
        }
        self.fading = None;
        self.linger = Some(cx.spawn(async move |this, cx| {
            this.update(cx, |this, cx| {
                if this.over != Some(spot) {
                    return;
                }
                this.hovered = Some(spot);
                cx.notify();
            })
            .ok();
        }));
    }

    fn enqueue(&mut self, pin: &Pin, gap: Option<usize>, cx: &mut Context<Self>) {
        self.playback
            .update(cx, |playback, cx| playback.enqueue_pin(pin, gap, cx));
    }

    fn dismiss_menu(&mut self, cx: &mut Context<Self>) {
        self.track_menu.reset(cx);
        self.context_menu = None;
        cx.notify();
    }

    fn row(
        track: Track,
        index: usize,
        position: QueuePosition,
        queue_revision: u64,
        look: RowLook,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let RowLook { playing, drop_line } = look;
        let theme = *cx.theme();
        let past_index = position.past();
        let queue_index = position.upcoming();
        let similar_index = position.similar();
        let title = match position {
            QueuePosition::Past(_) => theme.muted_foreground,
            QueuePosition::Current => theme.primary,
            QueuePosition::Upcoming(_) | QueuePosition::Similar(_) => theme.foreground,
        };
        let pin = track.pin();
        let menu_track = track.clone();

        let card = Card::new(
            ("queue-track", index),
            SharedString::from(track.name.clone()),
        )
        .cover(track.cover.clone())
        .bare_meta(
            crate::shared::cells::artist_links(
                SharedString::from(format!("queue-track-artist-{index}")),
                track.artist_refs.clone(),
                track.artists.clone(),
                theme.muted_foreground,
            )
            .text_size(theme.text(Text::Small))
            .truncate(),
        )
        .tint(title)
        .when(track.explicit, Card::explicit)
        .play(
            playing,
            cx.listener(move |this, _, _, cx| {
                let stale = this.queue.read(cx).revision() != queue_revision;
                this.playback.update(cx, |playback, cx| match position {
                    QueuePosition::Current => playback.toggle_play(cx),
                    QueuePosition::Past(index) if !stale => playback.play_past(index, cx),
                    QueuePosition::Upcoming(index) if !stale => playback.play_upcoming(index, cx),
                    QueuePosition::Similar(index) if !stale => playback.play_similar(index, cx),
                    _ => {}
                });
            }),
        )
        .menu(cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            this.track_menu.reset(cx);
            this.context_menu = Some(ContextMenuState {
                track: menu_track.clone(),
                revision: queue_revision,
                position: event.position,
            });
            cx.notify();
        }))
        .when_some(past_index, |this, index| {
            this.press(cx.listener(move |this, _, _, cx| {
                if this.queue.read(cx).revision() == queue_revision {
                    this.playback
                        .update(cx, |playback, cx| playback.play_past(index, cx));
                }
            }))
        })
        .when_some(queue_index, |this, target| {
            this.press(cx.listener(move |this, _, _, cx| {
                if this.queue.read(cx).revision() == queue_revision {
                    this.playback
                        .update(cx, |playback, cx| playback.play_upcoming(target, cx));
                }
            }))
            .action(
                Button::new(("remove-queued-track", index))
                    .ghost()
                    .small()
                    .mr_1()
                    .icon("icons/x.svg")
                    .tooltip("menu-remove-from-queue")
                    .tint(theme.muted_foreground)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.queue.update(cx, |queue, cx| {
                            if queue.revision() == queue_revision {
                                queue.remove_upcoming(target, cx);
                            }
                        });
                    })),
            )
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DraggedPin>, _, cx| {
                    let Some(gap) = drop_gap(event.bounds, event.event.position, target) else {
                        return;
                    };
                    let gap = match event.drag(cx).spot(QUEUE) {
                        Some(held) => (gap != held.index && gap != held.index + 1).then_some(gap),
                        None => Some(gap),
                    };
                    if this.drop_gap != gap {
                        this.drop_gap = gap;
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(move |this, dragged: &DraggedPin, _, cx| {
                let gap = this.drop_gap.take();
                match dragged.spot(QUEUE) {
                    Some(held) => {
                        if let Some(gap) = gap {
                            this.queue.update(cx, |queue, cx| {
                                if queue.revision() == held.revision {
                                    queue.move_upcoming_to_gap(held.index, gap, cx);
                                }
                            });
                        }
                    }
                    None => this.enqueue(&dragged.pin, gap, cx),
                }
                cx.notify();
            }))
        })
        .when_some(similar_index, |this, target| {
            this.press(cx.listener(move |this, _, _, cx| {
                if this.queue.read(cx).revision() == queue_revision {
                    this.playback
                        .update(cx, |playback, cx| playback.play_similar(target, cx));
                }
            }))
            .action(
                Button::new(("remove-similar-track", index))
                    .ghost()
                    .small()
                    .mr_1()
                    .icon("icons/x.svg")
                    .tooltip("menu-remove-from-queue")
                    .tint(theme.muted_foreground)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.queue.update(cx, |queue, cx| {
                            if queue.revision() == queue_revision {
                                queue.remove_similar(target, cx);
                            }
                        });
                    })),
            )
        })
        .when_some(pin, |this, pin| match queue_index {
            Some(index) => this.pin_from(pin, Spot::new(QUEUE, index).revision(queue_revision)),
            None => this.pin(pin),
        });

        div()
            .id(("queue-track-container", index))
            .relative()
            .min_w_0()
            .child(card)
            .when_some(drop_line, |this, edge| this.child(drop_marker(edge, cx)))
    }

    fn menu(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let ContextMenuState {
            track, position, ..
        } = self.context_menu.clone()?;

        Some(
            Popup::new(position, self.track_menu.for_track(&track, cx))
                .on_close(cx.listener(|this, _, _, cx| this.dismiss_menu(cx))),
        )
    }

    fn header(
        &self,
        sections: Sections,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = *cx.theme();

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_2()
            .h(snapped(theme.metrics.header, window))
            .px_2()
            .when(self.titled, |this| {
                this.border_b_1().border_color(theme.border).child(eyebrow(
                    match self.tab {
                        SideTab::Queue => t!("queue-title"),
                        SideTab::Lyrics => t!("lyrics-title"),
                    },
                    cx,
                ))
            })
            .when(!self.titled, |this| {
                this.justify_end().pr(theme.metrics.control + px(8.))
            })
            .when(self.tab == SideTab::Lyrics, |this| {
                this.children(self.sources(cx))
            })
            .when(self.tab == SideTab::Queue, |this| {
                this.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            Button::new("toggle-radio")
                                .ghost()
                                .small()
                                .icon("icons/radio.svg")
                                .tooltip("queue-radio")
                                .tint(match self.playback.read(cx).radio() {
                                    true => theme.primary,
                                    false => theme.muted_foreground,
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.playback
                                        .update(cx, |playback, cx| playback.toggle_radio(cx));
                                })),
                        )
                        .child(
                            Button::new("reset-queue")
                                .ghost()
                                .small()
                                .label(t!("queue-reset"))
                                .tint(theme.muted_foreground)
                                .disabled(!self.queue.read(cx).reordered())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.queue.update(cx, |queue, cx| queue.reset(cx));
                                })),
                        )
                        .child(
                            Button::new("clear-queue")
                                .ghost()
                                .small()
                                .label(t!("queue-clear"))
                                .tint(theme.muted_foreground)
                                .disabled(sections.upcoming == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.queue.update(cx, |queue, cx| queue.clear_upcoming(cx));
                                })),
                        ),
                )
            })
    }

    fn sources(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let lyrics = self.lyrics.read(cx);
        let chosen = lyrics.chosen();
        let found = lyrics
            .hits()
            .iter()
            .map(|hit| hit.source)
            .collect::<Vec<_>>();
        let current = *found.get(chosen)?;

        Some(
            Picker::new("lyrics-sources", &self.sources, current)
                .tooltip("lyrics-source-pick")
                .width(Picker::NARROW)
                .items(found.into_iter().enumerate().map(|(index, source)| {
                    MenuItem::new(("lyrics-source", index), source)
                        .selected(index == chosen)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics
                                .update(cx, |lyrics, cx| lyrics.choose(index, cx));
                        }))
                })),
        )
    }

    fn follow(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let theme = *cx.theme();
        if self.tab != SideTab::Lyrics || self.pinned {
            return None;
        }

        Some(
            div()
                .absolute()
                .when_else(self.titled, |this| this.bottom_3(), |this| this.bottom_16())
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div().flex().flex_none().block_mouse_except_scroll().child(
                        Button::new("resume-pin")
                            .ghost()
                            .small()
                            .icon("icons/undo-2.svg")
                            .tooltip("lyrics-follow")
                            .rounded_full()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.popover)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.anchor_verse();
                                cx.notify();
                            })),
                    ),
                ),
        )
    }

    fn ambience(&self, cx: &Context<Self>) -> Option<impl IntoElement> {
        self.ambience_of.as_ref()?;
        let veil = cx.theme().background;
        let scrim = veil.opacity(AMBIENCE_VEIL);

        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .child(self.ambience.clone())
                .child(div().absolute().inset_0().bg(scrim))
                .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                    180.,
                    gpui::linear_color_stop(veil.opacity(AMBIENCE_EDGE), 0.),
                    gpui::linear_color_stop(veil.opacity(0.), 0.45),
                )))
                .child(div().absolute().inset_0().bg(gpui::linear_gradient(
                    0.,
                    gpui::linear_color_stop(veil.opacity(AMBIENCE_EDGE), 0.),
                    gpui::linear_color_stop(veil.opacity(0.), 0.45),
                ))),
        )
    }

    fn verses(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let position = self.playback.read(cx).live_position();
        let singing = matches!(self.playback.read(cx).state(), PlaybackState::Playing);
        let lyrics = self.lyrics.read(cx);
        let state = lyrics.state().clone();
        let shown = lyrics.current().map(|hit| hit.lyrics.clone());
        let credit = lyrics
            .current()
            .map(|hit| (hit.source, hit.writers.clone()));
        let following = lyrics.following().map(str::to_owned);
        let take = lyrics.revision();
        let (karaoke_lyrics, romanization_scripts) = {
            let settings = self.settings.read(cx);
            (
                settings.karaoke_lyrics(),
                settings
                    .romanized_lyrics()
                    .then(|| settings.romanization_scripts()),
            )
        };
        let karaoke_effects = karaoke_lyrics && effects();
        let scale = match self.titled {
            true => self.settings.read(cx).panel_lyrics_scale(),
            false => self.settings.read(cx).fullscreen_lyrics_scale(),
        };
        let lane_size = theme.text(Text::Body) * scale;
        let sung = Sung {
            karaoke: karaoke_effects,
            lane: lane_size,
            scripts: romanization_scripts,
            theme,
            karaoke_tint: theme.foreground,
            motion: karaoke_effects && ui::motion::animates(cx),
        };

        if self.verse_of != following {
            self.verse_of = following;
            self.verse_take = take;
            self.forget_verse();
            self.anchor_verse();
            let scroll = self.verse_bar.read(cx).scroll().clone();
            scroll.set_offset(gpui::point(scroll.offset().x, px(0.)));
            self.verse_bar
                .update(cx, |bar, _| bar.remember_offset(scroll.offset().y));
        } else if self.verse_take != take {
            self.verse_take = take;
            // Nothing was on screen before, so there is no change to play: the
            // sheet is simply put up.
            self.resolving = self.showed;
            self.forget_verse();
            self.anchor_verse();
        }
        self.showed = shown.is_some();

        let empty = |key: &'static str, cx: &mut Context<Self>| {
            vacant(i18n::lookup(key, None), cx)
                .flex_1()
                .into_any_element()
        };
        let lines = match (&state, &shown) {
            (LyricsState::Ready, Some(music::Lyrics::Synced { lines })) => Some(lines.clone()),
            _ => None,
        };

        // aim before reading
        if let Some(lines) = &lines {
            let live = active_lyrics_row(lines, position);
            let focus = match self.pinning {
                Some(row) if Some(row) != live => Some(row),
                _ => {
                    self.pinning = None;
                    live
                }
            };
            self.pin_verse(focus, window, cx);
        }

        let verse = match self.titled {
            true => theme.text(Text::Large),
            false => theme.text(Text::Title) + FULLSCREEN_VERSE_GROWTH,
        } * scale;
        let reach = verse * REACH;
        let scroll = self.verse_bar.read(cx).scroll().clone();
        let (nudges, presentation) = {
            let bar = self.verse_bar.read(cx);
            (bar.nudges(), bar.presentation().y)
        };
        let animations = ui::motion::animates(cx);
        if !animations {
            self.drifts.clear();
        }
        let drag = match (lines.is_some(), animations) {
            (true, true) => self.lagged(&scroll, presentation, verse, nudges),
            _ => Drag::default(),
        };

        let mut body: Vec<gpui::AnyElement> = match (&lines, &state) {
            (Some(lines), _) => {
                let active_line = sung_line(lines, position);
                if singing
                    && karaoke_effects
                    && lines.iter().enumerate().any(|(index, line)| {
                        line.worded()
                            && primary_karaoke_visible(line, Some(index) == active_line, position)
                    })
                {
                    match window.is_window_active() {
                        true => window.request_animation_frame(),
                        false => self.sweep_karaoke(window, cx),
                    }
                }
                if self.previous_active_line != active_line {
                    if self.previous_active_line.is_some() {
                        self.departing_line = self.previous_active_line;
                        self.departure = self.departure.wrapping_add(1);
                        self.departed = std::time::Instant::now();
                    }
                    if active_line.is_some() {
                        self.arrival = self.arrival.wrapping_add(1);
                        self.arrived = std::time::Instant::now();
                    }
                    self.previous_active_line = active_line;
                }
                if self.departing_line.is_some() && self.departed.elapsed() >= Motion::Base.span() {
                    self.departing_line = None;
                }
                let instrumental_line = active_instrumental(lines, position);
                let hazing = effects() && self.pinned;
                let blur = verse * BLUR;
                let sharpen = self.sharpen_progress(window);
                // with motion turned down a press is simply on or off
                let sink = match animations {
                    true => self.sink_progress(window),
                    false => 1.,
                };
                let view = scroll.bounds();
                if hazing && scroll.bounds_for_item(0).is_none() {
                    window.request_animation_frame();
                }
                let mut rendered = Vec::with_capacity(lyric_row_count(lines));

                for (index, line) in lines.iter().enumerate() {
                    let seek = line.start;
                    let gap = instrumental_gap_before(lines, index);
                    let instrumental_start = line.start.saturating_sub(gap);
                    let has_instrumental = gap >= INSTRUMENTAL_BREAK;
                    let instrumental_progress = if has_instrumental {
                        progress_between(instrumental_start, line.start, position)
                    } else {
                        0.
                    };
                    let instrumental_has_passed = position >= line.start;

                    let verse_touch = self.touch(Warm::Verse(index), sharpen, sink);
                    let notes_touch = self.touch(Warm::Break(index), sharpen, sink);
                    let warmth = verse_touch.warmth;
                    let depth = verse_touch.depth;
                    // whatever the pointer rests on comes back into focus
                    let clearing = |touch: Touch, depth: f32| match (touch.waking, touch.settling) {
                        (true, _) => depth * (1. - sharpen),
                        (false, true) => depth * sharpen,
                        (false, false) => depth,
                    };
                    let haze = |depth: f32| clearing(verse_touch, depth);
                    if has_instrumental {
                        let notes_row = rendered.len();
                        if singing && instrumental_line == Some(index) {
                            window.request_animation_frame();
                        }
                        let notes_along = viewport_along(&scroll, notes_row, view, drag.downward);
                        let notes_drift = self.dragged(notes_row, notes_along, drag, window);
                        let notes_translation = presentation + notes_drift;
                        let softness = match hazing && instrumental_line != Some(index) {
                            true => clearing(
                                notes_touch,
                                viewport_haze(&scroll, notes_row, view, blur, notes_translation),
                            ),
                            false => 0.,
                        };
                        let notes = instrumental_row(
                            instrumental_progress,
                            instrumental_has_passed,
                            verse,
                            &theme,
                        )
                        .id(("instrumental", index))
                        .w_full()
                        .max_w(reach)
                        .px_2()
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .when(notes_touch.warmth > 0., |this| {
                            this.bg(theme.table_hover.opacity(notes_touch.warmth))
                        })
                        .when(notes_touch.depth > 0., |this| {
                            this.layer_scale(1. - (1. - PRESSED) * notes_touch.depth)
                        })
                        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                            this.set_hovered(Warm::Break(index), *over, cx)
                        }))
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Break(index), true, cx)
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Break(index), false, cx)
                            }),
                        )
                        .on_mouse_up_out(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Break(index), false, cx)
                            }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.seek_verse(notes_row, instrumental_start, cx);
                        }))
                        .when(softness > 0., |this| this.opacity(1. - VEIL * softness));
                        rendered.push(
                            adrift(notes, notes_translation, px(0.), window).into_any_element(),
                        );
                    }

                    let row = rendered.len();
                    let along = viewport_along(&scroll, row, view, drag.downward);
                    let drift = self.dragged(row, along, drag, window);
                    let translation = presentation + drift;
                    let active = Some(index) == active_line;
                    let departing = Some(index) == self.departing_line;
                    let karaoke = karaoke_effects
                        && line.worded()
                        && primary_karaoke_visible(line, active, position);
                    let primary_karaoke = karaoke && line.words.is_some();
                    let line_has_ended = active_line.is_some_and(|active| index < active)
                        || line_has_passed(line, position);
                    let worded = karaoke_effects && line.worded() && line.words.is_some();
                    let shade = |singing: bool| match (singing, line_has_ended) {
                        (true, _) if worded => theme.muted_foreground,
                        (true, _) => theme.foreground,
                        (false, true) => theme.muted_foreground.opacity(PAST),
                        (false, false) => theme.muted_foreground.opacity(AHEAD),
                    };
                    let tint = shade(Some(index) == active_line);

                    let dimming = (animations && departing).then_some(self.departure);
                    let growing =
                        animations && active && self.arrived.elapsed() < Motion::Base.span();
                    let shrinking = dimming.is_some();
                    let paint = match (growing, shrinking) {
                        (true, _) => mix(shade(false), tint, ramp(self.arrived, window)),
                        (_, true) => mix(shade(true), tint, ramp(self.departed, window)),
                        _ => tint,
                    };
                    let sung = Sung {
                        karaoke_tint: mix(
                            theme.foreground,
                            tint,
                            primary_karaoke_fade(line, active, position),
                        ),
                        ..sung
                    };

                    let primary = match (primary_karaoke, line.words.as_deref()) {
                        (true, Some(words)) => voiced_line(
                            &line.text,
                            words,
                            line.start,
                            position,
                            Voiced {
                                size: verse,
                                base: theme.muted_foreground,
                                top: sung.karaoke_tint,
                                soft: false,
                            },
                            !line.voice.lead(),
                            sung,
                        )
                        .into_any_element(),
                        _ => div()
                            .child(SharedString::from(line.text.clone()))
                            .into_any_element(),
                    };

                    let lanes =
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(line.secondary.iter().map(|lane| {
                                secondary_lyrics_lane(
                                    lane,
                                    line_has_ended,
                                    position,
                                    line.voice,
                                    sung,
                                )
                            }));
                    let content = div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .when(!line.voice.lead(), |this| this.items_end().text_right())
                        .child(primary)
                        .when_some(
                            selected_romanization(&line.romanized, romanization_scripts),
                            |this, text| this.child(romanized_lyrics_lane(text, lane_size, &theme)),
                        )
                        .when(!line.secondary.is_empty(), |this| this.child(lanes))
                        .when(depth > 0., |this| {
                            let room = halo_room(verse);
                            this.mx(-room)
                                .my(-room)
                                .p(room)
                                .layer_scale(1. - (1. - PRESSED) * depth)
                        });

                    let softness = match hazing && Some(index) != active_line {
                        true => haze(viewport_haze(&scroll, row, view, blur, translation)),
                        false => 0.,
                    };
                    let traded = index
                        .checked_sub(1)
                        .is_some_and(|previous| lines[previous].voice != line.voice);
                    let verse_line = div()
                        .id(("verse", index))
                        .w_full()
                        .max_w(reach)
                        .px_2()
                        .py_1()
                        .when(traded, |this| this.mt_2())
                        .rounded(theme.radius)
                        .cursor_pointer()
                        .when(warmth > 0., |this| {
                            this.bg(theme.table_hover.opacity(warmth))
                        })
                        .on_mouse_down(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Verse(index), true, cx)
                            }),
                        )
                        .on_mouse_up(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Verse(index), false, cx)
                            }),
                        )
                        .on_mouse_up_out(
                            gpui::MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.press_verse(Warm::Verse(index), false, cx)
                            }),
                        )
                        .text_size(verse)
                        .line_height(active_verse_size(verse) * ui::LEADING)
                        .text_color(tint)
                        .font_weight(FontWeight::SEMIBOLD)
                        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                            this.set_hovered(Warm::Verse(index), *over, cx)
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.seek_verse(row, seek, cx);
                        }))
                        .child(content);

                    let verse_line =
                        verse_line.when(softness > 0., |this| this.opacity(1. - VEIL * softness));
                    let verse_line = match (growing, shrinking, active) {
                        (_, true, false) | (true, _, _) | (_, _, true) => {
                            verse_line.text_color(paint)
                        }
                        _ => verse_line,
                    };
                    rendered.push(
                        adrift(verse_line, translation, halo_room(verse), window)
                            .into_any_element(),
                    );
                }

                rendered
            }
            (None, LyricsState::Ready) => match &shown {
                Some(music::Lyrics::Plain { text, romanized }) => vec![
                    div()
                        .w_full()
                        .max_w(reach)
                        .px_2()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .text_size(lane_size)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(SharedString::from(text.clone()))
                        .when_some(
                            selected_romanization(romanized, romanization_scripts),
                            |this, text| this.child(romanized_lyrics_lane(text, lane_size, &theme)),
                        )
                        .into_any_element(),
                ],
                _ => vec![wordless("lyrics-missing", "icons/mic-off.svg")],
            },
            (None, LyricsState::Idle) => vec![empty("lyrics-idle", cx)],
            (None, LyricsState::Loading) => vec![empty("lyrics-loading", cx)],
            (None, LyricsState::Instrumental) => {
                vec![wordless("lyrics-instrumental", "icons/guitar.svg")]
            }
            (None, LyricsState::Missing) => {
                vec![wordless("lyrics-missing", "icons/mic-off.svg")]
            }
            (None, LyricsState::Failed(_)) => vec![empty("lyrics-failed", cx)],
        };

        if state == LyricsState::Ready
            && let Some((source, writers)) = &credit
        {
            let credit = body.len();
            let along = viewport_along(&scroll, credit, scroll.bounds(), drag.downward);
            let drift = self.dragged(credit, along, drag, window);
            let translation = match lines.is_some() {
                true => presentation + drift,
                false => px(0.),
            };
            let note = div()
                .w_full()
                .max_w(reach)
                .px_2()
                .pt_2()
                .flex()
                .flex_col()
                .text_size(theme.text(Text::Small))
                .text_color(theme.muted_foreground)
                .child(t!("lyrics-source", source = *source))
                .when(!writers.is_empty(), |this| {
                    let writers = writers.join(", ");
                    this.child(t!("lyrics-writers", writers = writers.as_str()))
                });
            body.push(adrift(note, translation, px(0.), window).into_any_element());
        }

        let (over, under) = match &lines {
            Some(lines) => self.verse_slack(lyric_row_count(lines), window, cx),
            None => (px(REST), px(REST)),
        };

        let sheet = Scroller::new("lyrics", &self.verse_bar)
            .when(lines.is_some(), Scroller::manual_presentation)
            .face(ui::Face::Display)
            .flex()
            .flex_col()
            .items_center()
            .gap_4()
            .flex_1()
            .min_h_0()
            .px_1()
            .pt(over)
            .pb(under)
            .when(effects(), |this| {
                let fade = verse * VERSE_FADE;
                this.fade_edges(fade, fade)
            })
            .children(body);

        // A sheet only ever replaces another once, when every source has
        // answered, and that is the one change worth showing.
        match self.resolving && ui::motion::animates(cx) {
            true => sheet
                .with_animation(
                    ("verse-sheet", self.verse_take as usize),
                    Animation::new(Motion::Base.span()).with_easing(ui::ease_in_out_cubic),
                    move |this, t| {
                        this.blur(verse * RESOLVE_BLUR * (1. - t))
                            .opacity(1. - RESOLVE_FADE * (1. - t))
                    },
                )
                .into_any_element(),
            false => sheet.into_any_element(),
        }
    }

    fn verse_slack(&self, count: usize, window: &Window, cx: &App) -> (Pixels, Pixels) {
        let scroll = self.verse_bar.read(cx).scroll().clone();
        let view = scroll.bounds().size.height;
        if view <= px(0.) {
            window.request_animation_frame();
            return (px(REST), px(REST));
        }
        let tail = count
            .checked_sub(1)
            .and_then(|last| scroll.bounds_for_item(last))
            .map_or(px(0.), |item| item.size.height);

        (
            snapped((view * PIN).max(px(REST)), window),
            snapped((view * (1. - PIN) - tail).max(px(REST)), window),
        )
    }

    fn anchor_verse(&mut self) {
        self.pinned = true;
        self.aiming = false;
        self.rested = None;
        self.followed = None;
        self.nudged = None;
    }

    /// Seeks to a verse and holds the panel on the row it was asked for. The
    /// clock takes a moment to report the new position, and until it does the
    /// verse being sung is still the old one, which is where the panel would
    /// otherwise fly off to.
    fn seek_verse(&mut self, row: usize, position: std::time::Duration, cx: &mut Context<Self>) {
        self.pinning = Some(row);
        self.seek_lyrics(position, cx);
    }

    fn seek_lyrics(&mut self, position: std::time::Duration, cx: &mut Context<Self>) {
        self.anchor_verse();
        self.playback
            .update(cx, |playback, cx| playback.seek(position, cx));
        cx.notify();
    }

    fn pin_verse(&mut self, sung: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let scroll = self.verse_bar.read(cx).scroll().clone();
        let resting = scroll.offset().y;
        let nudges = self.verse_bar.read(cx).nudges();
        if self.nudges != nudges {
            self.nudges = nudges;
            self.pinned = false;
            self.flying = false;
            self.drifts.clear();
            self.nudged = Some(std::time::Instant::now());
        }
        if !self.pinned {
            self.followed = sung;
            // Keep the reader in charge for as long as they keep moving: the timer counts from the
            // last scroll, not from the first one.
            if self.rested != Some(resting) {
                self.rested = Some(resting);
                self.nudged = Some(std::time::Instant::now());
            }
            if self.nudged.is_some_and(|at| at.elapsed() >= SETTLE) {
                self.anchor_verse();
            } else {
                return;
            }
        }
        if sung.is_none() {
            return;
        }
        // The rows a verse sits among change on the very frame it starts being sung, and their
        // bounds only settle once that frame has been laid out. Aim on the next one.
        if self.followed != sung {
            self.followed = sung;
            self.aiming = true;
            window.request_animation_frame();
            return;
        }
        if !self.aiming {
            return;
        }
        let Some(item) = sung.and_then(|index| scroll.bounds_for_item(index)) else {
            return;
        };
        self.aiming = false;
        let view = scroll.bounds();
        // Preserve the fractional target. Spring scrolls are presented by the compositor, so the
        // text never has to walk the raster grid while the layer is settling.
        let goal = anchored_lyrics_offset(
            view.origin.y,
            item.origin.y,
            view.size.height,
            scroll.max_offset().y,
        );
        self.flown(goal, scroll.offset().y);
        match std::mem::take(&mut self.placing) || cx.reduce_motion() {
            true => self.verse_bar.update(cx, |bar, _| bar.place(goal)),
            false => self.verse_bar.update(cx, |bar, _| bar.aim(goal, window)),
        }
    }

    fn pin(&mut self, sections: Sections, window: &Window, cx: &Context<Self>) {
        let Some(index) = sections.current_index() else {
            self.anchor = false;
            return;
        };

        let viewport = self.scroll.0.borrow().base_handle.bounds().size.height;
        if viewport <= px(0.) {
            window.request_animation_frame();
            return;
        }

        let row = snapped(cx.theme().metrics.list_row, window);
        let above = (viewport * PINNED_SHARE / row).round() as usize;
        self.scroll
            .scroll_to_item_strict_with_offset(index, ScrollStrategy::Top, above);
        self.anchor = false;
    }

    // unnamed origins stay unlabelled
    fn playing_from(&self, cx: &App) -> Option<(SharedString, Destination)> {
        let origin = self.playback.read(cx).origin()?;
        let id = SharedString::from(origin.id.clone());
        let place = match origin.whence {
            Whence::Album => Destination::Album(id),
            Whence::Playlist => Destination::Playlist(id),
            Whence::Artist => Destination::Artist(id),
            Whence::Radio => Destination::Song(id),
            Whence::Saved => Destination::Library(LibraryTab::Songs),
            Whence::Local => match origin.id.is_empty() {
                true => Destination::Local(LocalTab::Songs),
                false => Destination::Local(LocalTab::Favorites),
            },
        };
        let name = match origin.whence {
            Whence::Saved => t!("library-liked-songs"),
            Whence::Local => match origin.id.is_empty() {
                true => t!("nav-local"),
                false => t!("library-liked-songs"),
            },
            _ => origin.name.clone()?,
        };

        Some((name, place))
    }

    fn rows(&self, sections: Sections, cx: &mut Context<Self>) -> gpui::UniformList {
        let queue = self.queue.clone();
        let from = self.playing_from(cx);
        let drop_gap = self.drop_gap;
        let upcoming = sections.upcoming;
        let audible = matches!(self.playback.read(cx).state(), PlaybackState::Playing);

        uniform_list(
            "queue-rows",
            sections.len() + TAIL_ROWS,
            cx.processor(move |_, range: Range<usize>, window, cx| {
                let (revision, slots) = {
                    let queue = queue.read(cx);
                    let slots = range
                        .clone()
                        .map(|index| {
                            let slot = (index < sections.len()).then(|| sections.slot(index));
                            let found = match slot {
                                Some(Slot::Track(position)) => track(queue, position),
                                Some(Slot::Header(_)) | None => None,
                            };
                            (index, slot, found)
                        })
                        .collect::<Vec<_>>();
                    (queue.revision(), slots)
                };

                slots
                    .into_iter()
                    .map(|(index, slot, found)| match (slot, found) {
                        (None, _) => div().into_any_element(),
                        (Some(Slot::Header(key)), _) => {
                            let label = section_label(key, window, cx);
                            match (key, from.clone()) {
                                ("queue-now-playing", Some((name, place))) => label
                                    .w_full()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_size(cx.theme().text(Text::Small))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(BULLET),
                                    )
                                    .child(faint(cx).child(t!("queue-from")))
                                    .child(source_link(name, place, cx))
                                    .into_any_element(),
                                _ => label.into_any_element(),
                            }
                        }
                        (Some(Slot::Track(position)), Some(found)) => {
                            let drop_line = match (position.upcoming(), drop_gap) {
                                (Some(queued), Some(gap)) if gap == queued => Some(Edge::Above),
                                (Some(queued), Some(gap))
                                    if gap == upcoming && queued + 1 == upcoming =>
                                {
                                    Some(Edge::Below)
                                }
                                _ => None,
                            };
                            let playing = audible && position == QueuePosition::Current;
                            let look = RowLook { playing, drop_line };
                            Self::row(found, index, position, revision, look, cx).into_any_element()
                        }
                        (Some(Slot::Track(_)), None) => div().into_any_element(),
                    })
                    .collect()
            }),
        )
    }
}

impl Render for Aside {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scrollbar.read(cx).sync();
        let queue = self.queue.read(cx);
        let sections = Sections {
            past: queue.past().len(),
            current: queue.current().is_some(),
            upcoming: queue.upcoming().len(),
            similar: queue.similar().len(),
        };
        let empty = sections.len() == 0;
        if !cx.has_active_drag() {
            self.drop_gap = None;
        }

        if self.past_len != sections.past {
            self.past_len = sections.past;
            self.anchor = true;
        }
        if self.anchor && self.tab == SideTab::Queue {
            self.pin(sections, window, cx);
        }

        let cover = self
            .playback
            .read(cx)
            .track()
            .and_then(|it| it.cover.clone());
        if self.ambience_of != cover {
            self.ambience_of = cover.clone();
            self.ambience.update(cx, |fluid, cx| fluid.paint(cover, cx));
        }

        div()
            .id("aside")
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .min_w_0()
            .when(self.tab == SideTab::Lyrics && self.titled, |this| {
                this.children(self.ambience(cx))
            })
            .on_drag_move(cx.listener(|this, _: &DragMoveEvent<DraggedPin>, _, cx| {
                if this.drop_gap.take().is_some() {
                    cx.notify();
                }
            }))
            .child(self.header(sections, window, cx))
            .child(
                div()
                    .id("queue-drop")
                    .relative()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .when(self.tab == SideTab::Queue, |this| {
                        this.on_drop(cx.listener(|this, dragged: &DraggedPin, _, cx| {
                            let gap = this.drop_gap.take();
                            if dragged.spot(QUEUE).is_none() {
                                this.enqueue(&dragged.pin, gap, cx);
                            }
                            cx.notify();
                        }))
                    })
                    .when(self.tab == SideTab::Lyrics, |this| {
                        this.child(self.verses(window, cx))
                    })
                    .when(self.tab == SideTab::Queue && empty, |this| {
                        this.child(vacant(t!("queue-empty"), cx).flex_1())
                    })
                    .when(self.tab == SideTab::Queue && !empty, |this| {
                        let gliding = self.scrollbar.clone();

                        this.child(
                            div()
                                .relative()
                                .flex_1()
                                .min_h_0()
                                .child(
                                    div()
                                        .size_full()
                                        .when(effects(), |this| {
                                            this.fade_edges(px(FADE * 0.5), px(FADE))
                                        })
                                        .child(
                                            self.rows(sections, cx)
                                                .px_2()
                                                .pt(px(FADE * 0.5))
                                                .track_scroll(&self.scroll)
                                                .size_full()
                                                .on_scroll_wheel(
                                                    move |event: &ScrollWheelEvent, window, cx| {
                                                        if event.delta.precise() {
                                                            return;
                                                        }
                                                        gliding
                                                            .update(cx, |bar, _| bar.nudge(window));
                                                    },
                                                ),
                                        ),
                                )
                                .child(self.scrollbar.clone()),
                        )
                    })
                    .children(self.follow(cx)),
            )
            .children(self.menu(cx))
    }
}

fn source_link(name: SharedString, to: Destination, cx: &App) -> impl IntoElement {
    let theme = *cx.theme();

    div()
        .id("queue-source")
        .min_w_0()
        .flex_shrink(1.)
        .truncate()
        .text_size(theme.text(Text::Small))
        .text_color(theme.muted_foreground)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(|style| style.text_color(theme.foreground).underline())
        .link(to)
        .child(name)
}

#[derive(Clone, Copy)]
struct Voiced {
    size: Pixels,
    base: gpui::Hsla,
    top: gpui::Hsla,
    // a backing lane moves and shines less
    soft: bool,
}

// the words, and a glow under them on a padded layer so the blur has room
fn voiced_line(
    text: &str,
    words: &[music::LyricsWord],
    start: std::time::Duration,
    position: std::time::Duration,
    voiced: Voiced,
    right: bool,
    sung: Sung,
) -> Div {
    let (text, look) = letters(text, words, start, position, voiced, sung.motion);
    let room = halo_room(voiced.size);
    div()
        .relative()
        .when(effects(), |this| {
            this.child(
                div()
                    .absolute()
                    .top(-room)
                    .bottom(-room)
                    .left(-room)
                    .right(-room)
                    .p(room)
                    .blur(voiced.size * GLOW_BLUR)
                    .child(
                        Verse::new(text.clone(), look.clone())
                            .right(right)
                            .glow(sung.theme.foreground),
                    ),
            )
        })
        .child(Verse::new(text, look).right(right))
}

// Every letter lights up in turn. A word held long enough sends a wave through
// its letters and pulses; any other word rises a little as a whole and settles.
fn letters(
    line: &str,
    words: &[music::LyricsWord],
    start: std::time::Duration,
    position: std::time::Duration,
    voiced: Voiced,
    motion: bool,
) -> (SharedString, Look) {
    let parts = karaoke_parts(line, words);
    let text = parts
        .iter()
        .map(|(piece, _)| piece.as_str())
        .collect::<String>();
    let mut counts = vec![0usize; words.len()];
    let mut slots = Vec::with_capacity(text.len());
    for (piece, word) in &parts {
        for letter in piece.chars() {
            let slot = match letter.is_whitespace() {
                true => None,
                false => {
                    counts[*word] += 1;
                    Some((*word, counts[*word] - 1))
                }
            };
            slots.extend(std::iter::repeat_n(slot, letter.len_utf8()));
        }
    }
    let windows = (0..words.len())
        .map(|word| karaoke_window(start, words, word))
        .collect::<Vec<_>>();
    let (reach, shine) = match voiced.soft {
        true => (SOFT, SOFT),
        false => (1., 1.),
    };
    let size = voiced.size;
    let rest = verse::Letter {
        color: voiced.base,
        lift: px(0.),
        glow: 0.,
    };
    let look: Look = std::rc::Rc::new(move |byte| {
        let Some(Some((word, letter))) = slots.get(byte).copied() else {
            return rest;
        };
        let (from, to) = windows[word];
        let count = counts[word] as f32;
        let lit = (progress_between(from, to, position) * count - letter as f32).clamp(0., 1.);
        let since = position.as_secs_f32() - from.as_secs_f32();
        let after = position.as_secs_f32() - to.as_secs_f32();
        let (lift, glow) = match to - from >= HELD {
            true => {
                let along = progress_between(from, to, position) * count - letter as f32;
                let tail = (count * WAVE_TAIL).max(WAVE_LEAST);
                let lift = match along > 0. && along < tail {
                    true => {
                        let wave = along / tail;
                        (wave * std::f32::consts::PI).sin() * (1. - wave * WAVE_EASE)
                    }
                    false => 0.,
                };
                let pulse = match since > 0. {
                    true => {
                        (1. - PULSE + PULSE * (since / PULSE_BEAT).sin())
                            * (1. - (after / GLOW_HELD_FADE).clamp(0., 1.))
                    }
                    false => 0.,
                };
                (size * WAVE * lift, lit * pulse * GLOW_HELD)
            }
            false => {
                let risen = ease_out_cubic((since / NUDGE_RISE).clamp(0., 1.));
                let back = ease_out_cubic((after / NUDGE_SETTLE).clamp(0., 1.));
                let gleam =
                    (since / GLOW_RISE).clamp(0., 1.) * (1. - (after / GLOW_FADE).clamp(0., 1.));
                (size * NUDGE * risen * (1. - back), lit * gleam * GLOW)
            }
        };
        verse::Letter {
            color: mix(voiced.base, voiced.top, lit),
            lift: match motion {
                true => lift * reach,
                false => px(0.),
            },
            glow: glow * shine,
        }
    });
    (text.into(), look)
}

fn secondary_lyrics_lane(
    lane: &music::LyricsLane,
    line_passed: bool,
    position: std::time::Duration,
    voice: Voice,
    sung: Sung,
) -> gpui::AnyElement {
    let theme = &sung.theme;
    let passed = line_passed || lane.sung_end().is_some_and(|end| position >= end);
    let singing = position >= lane.start
        && lane
            .sung_end()
            .is_none_or(|end| position < end + Motion::Control.span());
    let size = sung.lane;
    let lyrics =
        div().text_size(size).map(
            |this| match (sung.karaoke && singing, lane.words.as_deref()) {
                (true, Some(words)) if !words.is_empty() => this.child(voiced_line(
                    &lane.text,
                    words,
                    lane.start,
                    position,
                    Voiced {
                        size,
                        base: theme.muted_foreground.opacity(AHEAD),
                        top: theme.foreground.opacity(LANE_TOP),
                        soft: true,
                    },
                    !voice.lead(),
                    sung,
                )),
                _ => this
                    .text_color(match (singing, passed) {
                        (true, _) => theme.foreground.opacity(LANE_TOP),
                        (false, true) => theme.muted_foreground.opacity(PAST),
                        (false, false) => theme.muted_foreground.opacity(AHEAD),
                    })
                    .child(SharedString::from(lane.text.clone())),
            },
        );
    div()
        .flex()
        .flex_col()
        .when(!voice.lead(), |this| this.items_end().text_right())
        .child(lyrics)
        .when_some(
            selected_romanization(&lane.romanized, sung.scripts),
            |this, text| this.child(romanized_lyrics_lane(text, size, theme)),
        )
        .into_any_element()
}

fn selected_romanization(
    romanized: &Option<music::RomanizedText>,
    scripts: Option<RomanizationScripts>,
) -> Option<String> {
    let romanized = romanized.as_ref()?;
    scripts?
        .contains(romanized.writing_system)
        .then(|| romanized.text.clone())
}

fn romanized_lyrics_lane(text: String, size: Pixels, theme: &ui::Theme) -> Div {
    div()
        .text_size(size)
        .text_color(theme.muted_foreground)
        .child(SharedString::from(text))
}

fn karaoke_window(
    line_start: std::time::Duration,
    words: &[music::LyricsWord],
    index: usize,
) -> (std::time::Duration, std::time::Duration) {
    let word = &words[index];
    let start = match index {
        0 => line_start.min(word.start),
        _ => word.start,
    };
    let sung = word.end.max(start);
    let end = match words.get(index + 1) {
        // a rest after a word ends its sweep there rather than dragging it along
        Some(next) => match sung > start {
            true => next.start.max(start).min(sung),
            false => next.start.max(start),
        },
        None => sung,
    };
    (start, end)
}

#[cfg(test)]
fn karaoke_fragments(line: &str, words: &[music::LyricsWord]) -> Vec<String> {
    karaoke_parts(line, words)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

fn karaoke_parts(line: &str, words: &[music::LyricsWord]) -> Vec<(String, usize)> {
    let mut starts = Vec::with_capacity(words.len());
    let mut cursor = 0;
    for word in words {
        if word.text.is_empty() {
            return spaced_words(words);
        }
        let Some(remainder) = line.get(cursor..) else {
            return spaced_words(words);
        };
        let Some(relative) = remainder.find(&word.text) else {
            return spaced_words(words);
        };
        let start = cursor + relative;
        starts.push(start);
        cursor = start + word.text.len();
    }

    starts
        .iter()
        .enumerate()
        .flat_map(|(index, start)| {
            let start = match index {
                0 => 0,
                _ => *start,
            };
            let end = starts.get(index + 1).copied().unwrap_or(line.len());
            plain_lyrics_fragments(&line[start..end])
                .into_iter()
                .map(move |piece| (piece, index))
        })
        .collect()
}

fn spaced_words(words: &[music::LyricsWord]) -> Vec<(String, usize)> {
    words
        .iter()
        .enumerate()
        .flat_map(|(index, word)| {
            let mut text = word.text.clone();
            if words
                .get(index + 1)
                .is_some_and(|next| needs_space(&word.text, &next.text))
            {
                text.push(' ');
            }
            plain_lyrics_fragments(&text)
                .into_iter()
                .map(move |piece| (piece, index))
        })
        .collect()
}

fn needs_space(left: &str, right: &str) -> bool {
    let Some(last) = left.chars().next_back() else {
        return false;
    };
    let Some(first) = right.chars().next() else {
        return false;
    };
    if last.is_whitespace() || first.is_whitespace() {
        return false;
    }
    if wide(last) || wide(first) {
        return false;
    }
    !matches!(last, '(' | '[' | '{' | '\'' | '’' | '-' | '—')
        && !matches!(
            first,
            ')' | ']' | '}' | ',' | '.' | '!' | '?' | ';' | ':' | '%' | '\'' | '’' | '-' | '—'
        )
}

/// How far along a transition started at this moment is, asking for frames while
/// it runs.
fn ramp(at: std::time::Instant, window: &mut Window) -> f32 {
    let span = Motion::Base.span().as_secs_f32().max(f32::EPSILON);
    let progress = (at.elapsed().as_secs_f32() / span).clamp(0., 1.);
    if progress < 1. {
        window.request_animation_frame();
    }
    ease_out_expo(progress)
}

fn active_verse_size(verse: Pixels) -> Pixels {
    verse + ACTIVE_VERSE_GROWTH
}

fn plain_lyrics_fragments(line: &str) -> Vec<String> {
    let mut fragments = Vec::new();
    let mut start = 0;
    let mut spacing = false;
    let mut previous = None;
    for (index, letter) in line.char_indices() {
        if letter.is_whitespace() {
            spacing = true;
        } else if spacing || previous.is_some_and(|previous| parts(previous, letter)) {
            fragments.push(line[start..index].to_owned());
            start = index;
            spacing = false;
        }
        previous = Some(letter);
    }
    if start < line.len() {
        fragments.push(line[start..].to_owned());
    }
    fragments
}

fn wide(letter: char) -> bool {
    matches!(letter,
        '\u{2E80}'..='\u{303E}'
        | '\u{3041}'..='\u{33FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{A000}'..='\u{A4CF}'
        | '\u{AC00}'..='\u{D7AF}'
        | '\u{F900}'..='\u{FAFF}'
        | '\u{FF00}'..='\u{FF60}'
    )
}

fn parts(left: char, right: char) -> bool {
    if !wide(left) || !wide(right) {
        return false;
    }

    !matches!(
        right,
        '、' | '。'
            | '，'
            | '．'
            | '！'
            | '？'
            | '：'
            | '；'
            | '」'
            | '』'
            | '）'
            | '】'
            | '〉'
            | '》'
            | '〕'
            | '・'
            | 'ー'
            | '…'
            | '々'
            | 'ゝ'
            | 'ゞ'
            | 'っ'
            | 'ッ'
    ) && !matches!(left, '「' | '『' | '（' | '【' | '〈' | '《' | '〔')
}

fn anchored_lyrics_offset(view: Pixels, item: Pixels, height: Pixels, reach: Pixels) -> Pixels {
    let delta = view - item + height * PIN;
    delta.clamp(-reach, px(0.))
}

fn progress_between(
    start: std::time::Duration,
    end: std::time::Duration,
    position: std::time::Duration,
) -> f32 {
    if position < start {
        return 0.;
    }
    if position >= end {
        return 1.;
    }
    let span = (end - start).as_secs_f32();
    ((position - start).as_secs_f32() / span).clamp(0., 1.)
}

fn instrumental_gap_before(lines: &[music::LyricsLine], index: usize) -> std::time::Duration {
    let start = lines[index].start;
    match index {
        0 => start,
        _ => {
            let previous = &lines[index - 1];
            start.saturating_sub(previous.sung_end().unwrap_or(previous.start))
        }
    }
}

fn active_instrumental(
    lines: &[music::LyricsLine],
    position: std::time::Duration,
) -> Option<usize> {
    let next_line = lines.iter().position(|line| line.start > position)?;
    let gap = instrumental_gap_before(lines, next_line);
    let start = lines[next_line].start.saturating_sub(gap);
    (gap >= INSTRUMENTAL_BREAK && position >= start).then_some(next_line)
}

fn lyric_row_count(lines: &[music::LyricsLine]) -> usize {
    lines.len()
        + (0..lines.len())
            .filter(|index| instrumental_gap_before(lines, *index) >= INSTRUMENTAL_BREAK)
            .count()
}

fn line_row(lines: &[music::LyricsLine], index: usize) -> usize {
    index
        + (0..=index)
            .filter(|line| instrumental_gap_before(lines, *line) >= INSTRUMENTAL_BREAK)
            .count()
}

fn active_lyrics_row(lines: &[music::LyricsLine], position: std::time::Duration) -> Option<usize> {
    if let Some(index) = sung_line(lines, position) {
        return Some(line_row(lines, index));
    }
    let index = active_instrumental(lines, position)?;
    line_row(lines, index).checked_sub(1)
}

fn sung_line(lines: &[music::LyricsLine], position: std::time::Duration) -> Option<usize> {
    match active_instrumental(lines, position) {
        Some(_) => None,
        None => music::lyrics::active(lines, position),
    }
}

// culling needs layout
// A layer keeps only what it painted inside its own bounds, so it is padded: the
// glow around the words would otherwise be cut square at the row's edge.
fn adrift(row: impl IntoElement, shift: Pixels, room: Pixels, window: &Window) -> Div {
    let grid = snapped(shift, window);

    div().w_full().flex().flex_col().child(
        div()
            .mx(-room)
            .my(-room)
            .p(room)
            .flex()
            .flex_col()
            .items_center()
            .top(grid)
            .layer_translate(gpui::point(px(0.), shift - grid))
            .child(row),
    )
}

fn halo_room(verse: Pixels) -> Pixels {
    verse * (GLOW_BLUR * BLUR_REACH + WAVE)
}

struct Place {
    top: Pixels,
    height: Pixels,
    travel: f32,
    along: f32,
}

// shift is drawn, not laid out
fn viewport_place(
    scroll: &ScrollHandle,
    row: usize,
    view: Bounds<Pixels>,
    shift: Pixels,
) -> Option<Place> {
    let height = view.size.height;
    let item = scroll.bounds_for_item(row)?;
    if height <= px(0.) {
        return None;
    }
    let top = item.origin.y - view.origin.y + scroll.offset().y + shift;
    let travel = top - height * PIN;
    let reach = height
        * match travel >= px(0.) {
            true => 1. - PIN,
            false => PIN,
        };
    Some(Place {
        top,
        height: item.size.height,
        travel: (travel / reach.max(px(1.))).clamp(-1., 1.),
        along: (top / height).clamp(0., 1.),
    })
}

fn viewport_haze(
    scroll: &ScrollHandle,
    row: usize,
    view: Bounds<Pixels>,
    margin: Pixels,
    drift: Pixels,
) -> f32 {
    let Some(place) = viewport_place(scroll, row, view, drift) else {
        return 0.;
    };
    if place.top + place.height + margin < px(0.) || place.top - margin > view.size.height {
        return 0.;
    }
    ease_in_out(((place.travel.abs() - HAZE) / (1. - HAZE)).clamp(0., 1.))
}

#[derive(Clone, Copy, Default)]
struct Drag {
    step: Pixels,
    beat: f32,
    downward: bool,
    most: Pixels,
}

fn lag_spring(along: f32) -> SpringConfig {
    let frequency = 1. - LAG_STAGGER * along.clamp(0., 1.);
    let spring = Springs::LYRICS_ROW;
    SpringConfig::new(
        spring.stiffness * frequency * frequency,
        spring.damping * frequency,
        spring.mass,
    )
}

// incoming rows last
fn viewport_along(scroll: &ScrollHandle, row: usize, view: Bounds<Pixels>, downward: bool) -> f32 {
    let Some(place) = viewport_place(scroll, row, view, px(0.)) else {
        return 0.;
    };
    match downward {
        true => place.along,
        false => 1. - place.along,
    }
}

fn line_has_passed(line: &music::LyricsLine, position: std::time::Duration) -> bool {
    line.sung_end().is_some_and(|end| position >= end)
}

fn primary_karaoke_visible(
    line: &music::LyricsLine,
    line_active: bool,
    position: std::time::Duration,
) -> bool {
    line_active
        || (position >= line.start
            && line
                .sung_end()
                .is_some_and(|end| position < end + Motion::Control.span()))
}

fn primary_karaoke_fade(
    line: &music::LyricsLine,
    line_active: bool,
    position: std::time::Duration,
) -> f32 {
    if line_active {
        return 0.;
    }
    line.sung_end().map_or(0., |end| {
        progress_between(end, end + Motion::Control.span(), position)
    })
}

fn instrumental_row(progress: f32, past: bool, verse: Pixels, theme: &ui::Theme) -> Div {
    let note_size = verse * 1.;
    div()
        .flex()
        .items_center()
        .gap_2()
        .py(verse * 0.45)
        .children((0..3).map(|index| {
            let note_progress = (progress * 3. - index as f32).clamp(0., 1.);
            let tint = match past {
                true => theme.muted_foreground.opacity(PAST),
                false => mix(
                    theme.muted_foreground.opacity(AHEAD),
                    theme.primary,
                    note_progress,
                ),
            };
            div()
                .size(note_size)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(icons::path("icons/music-2.svg"))
                        .size(note_size)
                        .text_color(tint),
                )
        }))
}

fn wordless(key: &'static str, icon: &'static str) -> gpui::AnyElement {
    Vacancy::new(i18n::lookup(key, None))
        .icon(icon)
        .flex_1()
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use music::{LyricsLine, LyricsWord, Voice};
    use ui::Springs;

    use super::{
        QueuePosition, Sections, Slot, active_lyrics_row, anchored_lyrics_offset,
        karaoke_fragments, karaoke_window, lag_spring, line_has_passed, line_row, lyric_row_count,
        plain_lyrics_fragments, primary_karaoke_fade, primary_karaoke_visible,
    };
    use gpui::px;
    use ui::Motion;

    fn slots(sections: Sections) -> Vec<Slot> {
        (0..sections.len()).map(|i| sections.slot(i)).collect()
    }

    #[test]
    fn lays_out_every_section() {
        let sections = Sections {
            past: 2,
            current: true,
            upcoming: 2,
            similar: 2,
        };

        assert_eq!(sections.current_index(), Some(4));
        assert_eq!(
            slots(sections),
            [
                Slot::Header("queue-history"),
                Slot::Track(QueuePosition::Past(0)),
                Slot::Track(QueuePosition::Past(1)),
                Slot::Header("queue-now-playing"),
                Slot::Track(QueuePosition::Current),
                Slot::Header("queue-up-next"),
                Slot::Track(QueuePosition::Upcoming(0)),
                Slot::Track(QueuePosition::Upcoming(1)),
                Slot::Header("queue-similar"),
                Slot::Track(QueuePosition::Similar(0)),
                Slot::Track(QueuePosition::Similar(1)),
            ]
        );
    }

    #[test]
    fn suggests_similar_tracks_without_anything_up_next() {
        let sections = Sections {
            past: 0,
            current: true,
            upcoming: 0,
            similar: 1,
        };

        assert_eq!(
            slots(sections),
            [
                Slot::Header("queue-now-playing"),
                Slot::Track(QueuePosition::Current),
                Slot::Header("queue-similar"),
                Slot::Track(QueuePosition::Similar(0)),
            ]
        );
    }

    #[test]
    fn drops_headers_for_empty_sections() {
        let sections = Sections {
            past: 0,
            current: true,
            upcoming: 1,
            similar: 0,
        };

        assert_eq!(sections.current_index(), Some(1));
        assert_eq!(
            slots(sections),
            [
                Slot::Header("queue-now-playing"),
                Slot::Track(QueuePosition::Current),
                Slot::Header("queue-up-next"),
                Slot::Track(QueuePosition::Upcoming(0)),
            ]
        );
    }

    #[test]
    fn lays_out_history_without_a_current_track() {
        let sections = Sections {
            past: 1,
            current: false,
            upcoming: 0,
            similar: 0,
        };

        assert_eq!(sections.current_index(), None);
        assert_eq!(
            slots(sections),
            [
                Slot::Header("queue-history"),
                Slot::Track(QueuePosition::Past(0))
            ]
        );
    }

    #[test]
    fn an_empty_queue_has_no_rows() {
        let sections = Sections {
            past: 0,
            current: false,
            upcoming: 0,
            similar: 0,
        };

        assert_eq!(sections.len(), 0);
        assert_eq!(sections.current_index(), None);
    }

    #[test]
    fn a_long_instrumental_pause_gets_its_own_lyrics_row() {
        let lines = [
            LyricsLine {
                start: Duration::from_secs(2),
                end: Some(Duration::from_secs(5)),
                text: "first".to_owned(),
                romanized: None,
                words: None,
                secondary: Vec::new(),
                voice: Voice::Lead,
            },
            LyricsLine {
                start: Duration::from_secs(12),
                end: Some(Duration::from_secs(15)),
                text: "second".to_owned(),
                romanized: None,
                words: None,
                secondary: Vec::new(),
                voice: Voice::Lead,
            },
        ];

        assert_eq!(lyric_row_count(&lines), 3);
        assert_eq!(line_row(&lines, 0), 0);
        assert_eq!(line_row(&lines, 1), 2);
        assert_eq!(active_lyrics_row(&lines, Duration::from_secs(8)), Some(1));
        assert_eq!(active_lyrics_row(&lines, Duration::from_secs(13)), Some(2));
    }

    #[test]
    fn word_timing_exposes_a_pause_hidden_by_the_line_end() {
        let lines = [
            LyricsLine {
                start: Duration::from_secs(2),
                end: Some(Duration::from_secs(12)),
                text: "first".to_owned(),
                romanized: None,
                words: Some(vec![LyricsWord {
                    start: Duration::from_secs(2),
                    end: Duration::from_secs(5),
                    text: "first".to_owned(),
                }]),
                secondary: Vec::new(),
                voice: Voice::Lead,
            },
            LyricsLine {
                start: Duration::from_secs(12),
                end: Some(Duration::from_secs(15)),
                text: "second".to_owned(),
                romanized: None,
                words: None,
                secondary: Vec::new(),
                voice: Voice::Lead,
            },
        ];

        assert_eq!(lyric_row_count(&lines), 3);
        assert_eq!(active_lyrics_row(&lines, Duration::from_secs(8)), Some(1));
        assert!(line_has_passed(&lines[0], Duration::from_secs(8)));
    }

    #[test]
    fn lyrics_follow_uses_unscrolled_item_bounds() {
        let offset = anchored_lyrics_offset(px(0.), px(200.), px(100.), px(500.));

        assert_eq!(offset, px(-170.));
    }

    #[test]
    fn lyrics_follow_preserves_a_subpixel_target() {
        let offset = anchored_lyrics_offset(px(0.25), px(200.125), px(100.5), px(500.));

        assert!((offset.as_f32() - -169.725).abs() < 0.001);
    }

    #[test]
    fn karaoke_uses_spacing_from_the_complete_line() {
        let text = "I said oooh I'm drowning in the night";
        let words = ["I", "said", "oooh", "I'm", "drowning", "in", "the", "night"]
            .into_iter()
            .enumerate()
            .map(|(index, text)| LyricsWord {
                start: Duration::from_millis(index as u64 * 100),
                end: Duration::from_millis(index as u64 * 100 + 100),
                text: text.to_owned(),
            })
            .collect::<Vec<_>>();

        let fragments = karaoke_fragments(text, &words);

        assert_eq!(fragments.concat(), text);
        assert_eq!(
            fragments,
            [
                "I ",
                "said ",
                "oooh ",
                "I'm ",
                "drowning ",
                "in ",
                "the ",
                "night"
            ]
        );
    }

    #[test]
    fn plain_lyrics_keep_spacing_in_breakable_fragments() {
        let fragments = plain_lyrics_fragments("Ладони полны слёзок, но время");

        assert_eq!(fragments.concat(), "Ладони полны слёзок, но время");
        assert_eq!(fragments, ["Ладони ", "полны ", "слёзок, ", "но ", "время"]);
    }

    #[test]
    fn lyrics_row_springs_stagger_without_changing_their_damping_ratio() {
        let (first_frequency, first_ratio) = lag_spring(0.).canonical();
        let (last_frequency, last_ratio) = lag_spring(1.).canonical();

        assert!(first_frequency > last_frequency);
        assert!((first_ratio - last_ratio).abs() < f32::EPSILON);
        assert!(
            first_ratio < 1.,
            "the lyrics settle should have a subtle overshoot"
        );
    }

    #[test]
    fn lyrics_keep_their_tuned_spring_presets() {
        assert_eq!(Springs::LYRICS_SCROLL.stiffness, 170.);
        assert_eq!(Springs::LYRICS_SCROLL.damping, 23.);
        assert_eq!(Springs::LYRICS_SCROLL.mass, 1.);
        assert_eq!(Springs::LYRICS_ROW.stiffness, 210.);
        assert_eq!(Springs::LYRICS_ROW.damping, 22.);
        assert_eq!(Springs::LYRICS_ROW.mass, 1.);
    }

    #[test]
    fn a_late_first_word_uses_the_whole_lead_in() {
        let words = vec![
            LyricsWord {
                start: Duration::from_millis(1500),
                end: Duration::from_millis(1900),
                text: "first".to_owned(),
            },
            LyricsWord {
                start: Duration::from_millis(2000),
                end: Duration::from_millis(2400),
                text: "second".to_owned(),
            },
        ];

        assert_eq!(
            karaoke_window(Duration::from_millis(1000), &words, 0),
            (Duration::from_millis(1000), Duration::from_millis(1900))
        );
    }

    #[test]
    fn an_overlapped_primary_line_keeps_singing_in_the_background() {
        let line = LyricsLine {
            start: Duration::from_secs(2),
            end: Some(Duration::from_secs(8)),
            text: "Wake me up inside".to_owned(),
            romanized: None,
            words: Some(vec![LyricsWord {
                start: Duration::from_secs(2),
                end: Duration::from_secs(8),
                text: "Wake me up inside".to_owned(),
            }]),
            secondary: Vec::new(),
            voice: Voice::Lead,
        };

        assert!(!primary_karaoke_visible(
            &line,
            false,
            Duration::from_millis(1999)
        ));
        assert!(primary_karaoke_visible(
            &line,
            false,
            Duration::from_secs(5)
        ));
        assert!(primary_karaoke_visible(
            &line,
            false,
            Duration::from_secs(8)
        ));
        assert!(!primary_karaoke_visible(
            &line,
            false,
            Duration::from_secs(8) + Motion::Base.span()
        ));
    }

    #[test]
    fn the_active_primary_line_keeps_its_completed_sweep_until_departure() {
        let line = LyricsLine {
            start: Duration::from_secs(2),
            end: Some(Duration::from_secs(5)),
            text: "line".to_owned(),
            romanized: None,
            words: Some(vec![LyricsWord {
                start: Duration::from_secs(2),
                end: Duration::from_secs(5),
                text: "line".to_owned(),
            }]),
            secondary: Vec::new(),
            voice: Voice::Lead,
        };

        assert!(primary_karaoke_visible(&line, true, Duration::from_secs(8)));
    }

    #[test]
    fn a_finished_background_line_fades_from_white_to_gray() {
        let line = LyricsLine {
            start: Duration::from_secs(2),
            end: Some(Duration::from_secs(8)),
            text: "Wake me up inside".to_owned(),
            romanized: None,
            words: Some(vec![LyricsWord {
                start: Duration::from_secs(2),
                end: Duration::from_secs(8),
                text: "Wake me up inside".to_owned(),
            }]),
            secondary: Vec::new(),
            voice: Voice::Lead,
        };
        let fade = Motion::Control.span();

        assert_eq!(
            primary_karaoke_fade(&line, false, Duration::from_millis(7999)),
            0.
        );
        assert_eq!(
            primary_karaoke_fade(&line, false, Duration::from_secs(8) + fade / 2),
            0.5
        );
        assert_eq!(
            primary_karaoke_fade(&line, false, Duration::from_secs(8) + fade),
            1.
        );
        assert_eq!(
            primary_karaoke_fade(&line, true, Duration::from_secs(8) + fade),
            0.
        );
    }

    #[test]
    fn a_finished_line_stays_past_during_a_gap() {
        let line = LyricsLine {
            start: Duration::from_secs(2),
            end: Some(Duration::from_secs(5)),
            text: "line".to_owned(),
            romanized: None,
            words: None,
            secondary: Vec::new(),
            voice: Voice::Lead,
        };

        assert!(line_has_passed(&line, Duration::from_secs(8)));
    }
}
