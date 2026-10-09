use gpui::prelude::*;
use gpui::{
    AnyView, Context, Entity, EventEmitter, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, SharedString,
};
use gpui::{Window, div, point, px};
use i18n::t;
use ui::WindowControls;
use ui::{ActiveTheme as _, Button};
use ui::{FORM_CONTEXT, Input, Menu, MenuItem, Popup, Submit};

use crate::chrome::SidebarRight;
use router::{Destination, Navigation, navigate};
use state::{AppSettings, Hit, Kind, Playback, Search, Sonora};

const QUICK_ORDER: [(Kind, usize); 3] = [(Kind::Artist, 2), (Kind::Album, 3), (Kind::Song, 3)];

const SYSTEM_ZOOMS: bool = cfg!(target_os = "windows");
const SEARCH_WIDTH: Pixels = px(380.);

#[cfg(target_os = "macos")]
const TITLE_BAR_LEFT_INSET: f32 = 74.;
#[cfg(not(target_os = "macos"))]
const TITLE_BAR_LEFT_INSET: f32 = 12.;

#[derive(Clone, PartialEq)]
pub(crate) struct TitleBarOptions {
    pub navigation: bool,
    pub sidebar_open: bool,
    pub sidebar_right: Option<bool>,
    pub offset: Pixels,
    pub border: bool,
    pub content: Option<AnyView>,
}

impl Default for TitleBarOptions {
    fn default() -> Self {
        Self {
            navigation: false,
            sidebar_open: false,
            sidebar_right: None,
            offset: Pixels::ZERO,
            border: true,
            content: None,
        }
    }
}

pub(crate) enum TitleBarEvent {
    ToggleSidebar,
    ToggleSidebarRight,
    Search(SharedString),
}

pub(crate) struct TitleBar {
    navigation: Entity<Navigation>,
    settings: Entity<AppSettings>,
    search: Entity<Search>,
    playback: Entity<Playback>,
    options: TitleBarOptions,
    grabbed: bool,
    query: Entity<Input>,
    dismissed: bool,
}

impl EventEmitter<TitleBarEvent> for TitleBar {}

impl TitleBar {
    pub fn new(search: Entity<Search>, playback: Entity<Playback>, cx: &mut Context<Self>) -> Self {
        let navigation = router::trail(cx);
        let settings = Sonora::global(cx).settings.clone();
        let query = cx.new(|cx| {
            Input::new("common-search", cx)
                .icon("icons/search.svg")
                .compact()
                .clearable()
        });

        cx.observe(&navigation, |_, _, cx| cx.notify()).detach();
        cx.observe(&settings, |_, _, cx| cx.notify()).detach();
        cx.observe(&query, |this, query, cx| {
            this.dismissed = false;
            let text = query.read(cx).text().to_owned();
            this.search.update(cx, |search, cx| search.ask(&text, cx));
            cx.notify();
        })
        .detach();
        cx.observe(&search, |_, _, cx| cx.notify()).detach();
        Self {
            navigation,
            settings,
            search,
            playback,
            options: TitleBarOptions::default(),
            grabbed: false,
            query,
            dismissed: false,
        }
    }

    fn submit_search(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        let text = self.query.read(cx).text().trim().to_owned();
        if text.is_empty() {
            return;
        }
        self.dismissed = true;
        cx.emit(TitleBarEvent::Search(text.into()));
    }

    fn search_bar(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let left = ((window.viewport_size().width - SEARCH_WIDTH) / 2.).max(Pixels::ZERO);

        div()
            .absolute()
            .top_0()
            .left(left)
            .h_full()
            .w(SEARCH_WIDTH)
            .flex()
            .items_center()
            .occlude()
            .key_context(FORM_CONTEXT)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_action(cx.listener(Self::submit_search))
            .child(self.query.clone())
    }

    fn quick_results(&self, window: &Window, cx: &mut Context<Self>) -> Option<Popup> {
        if self.dismissed || self.query.read(cx).text().trim().is_empty() {
            return None;
        }

        let hits = quick_hits(self.search.read(cx));
        if hits.is_empty() {
            return None;
        }

        let left = ((window.viewport_size().width - SEARCH_WIDTH) / 2.).max(Pixels::ZERO);
        let top = ui::snapped(cx.theme().metrics.title_bar, window);
        let playback = self.playback.clone();

        let mut menu = Menu::new("quick-search").w(SEARCH_WIDTH);
        let mut heading = None;
        for (place, hit) in hits.iter().enumerate() {
            let kind = hit.kind();
            if heading != Some(kind) {
                heading = Some(kind);
                menu = menu.item(
                    MenuItem::new(("quick-heading", place), "")
                        .content(ui::eyebrow(quick_heading(kind), cx)),
                );
            }
            menu = menu.item(quick_item(place, hit, &playback));
        }

        Some(
            Popup::new(point(left, top), menu).on_close(cx.listener(|this, _, _, cx| {
                this.dismissed = true;
                cx.notify();
            })),
        )
    }

    pub fn set_options(&mut self, options: TitleBarOptions, cx: &mut Context<Self>) {
        if self.options == options {
            return;
        }
        self.options = options;
        cx.notify();
    }

    fn history(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hover = cx.theme().sidebar_accent;
        let navigation = self.navigation.read(cx);
        let (can_back, can_forward) = (navigation.can_go_back(), navigation.can_go_forward());
        let back = self.navigation.clone();
        let forward = self.navigation.clone();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("history-back")
                    .ghost()
                    .icon("icons/chevron-left.svg")
                    .tooltip("nav-back")
                    .disabled(!can_back)
                    .size_8()
                    .px_0()
                    .when(can_back, |button| {
                        button
                            .hover(move |style| style.bg(hover))
                            .active(move |style| style.bg(hover))
                    })
                    .on_click(move |_, _, cx| {
                        back.update(cx, |navigation, cx| navigation.back(cx))
                    }),
            )
            .child(
                Button::new("history-forward")
                    .ghost()
                    .icon("icons/chevron-right.svg")
                    .tooltip("nav-forward")
                    .disabled(!can_forward)
                    .size_8()
                    .px_0()
                    .when(can_forward, |button| {
                        button
                            .hover(move |style| style.bg(hover))
                            .active(move |style| style.bg(hover))
                    })
                    .on_click(move |_, _, cx| {
                        forward.update(cx, |navigation, cx| navigation.forward(cx))
                    }),
            )
    }

    fn lyrics_toggle(&self, open: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("sidebar-right-toggle")
                    .ghost()
                    .small()
                    .icon(match open {
                        true => "icons/panel-right-close.svg",
                        false => "icons/panel-right-open.svg",
                    })
                    .tooltip("nav-sidebar-right")
                    .selected(open)
                    .on_click(
                        cx.listener(|_, _, _, cx| cx.emit(TitleBarEvent::ToggleSidebarRight)),
                    ),
            )
    }

    fn toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let icon = match self.options.sidebar_open {
            true => "icons/panel-left-close.svg",
            false => "icons/panel-left-open.svg",
        };

        div()
            .flex()
            .flex_none()
            .items_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("sidebar-toggle")
                    .ghost()
                    .flex()
                    .small()
                    .icon(icon)
                    .tooltip("nav-sidebar")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(TitleBarEvent::ToggleSidebar))),
            )
    }
}

impl Render for TitleBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let height = ui::snapped(theme.metrics.title_bar, window);
        let navigation = self.options.navigation;
        let offset = match navigation {
            true => self.options.offset,
            false => Pixels::ZERO,
        };
        let content = self.options.content.clone();
        let settings = self.settings.read(cx);
        let decorated = cfg!(not(target_os = "macos")) && settings.window_controls();
        let leading = decorated && settings.controls_on_left();
        let on_search = matches!(self.navigation.read(cx).current(), Destination::Search);

        div()
            .relative()
            .flex()
            .items_center()
            .w_full()
            .h(height)
            .flex_none()
            .when(!theme.transparent, |this| this.bg(theme.background))
            .when(self.options.border, |this| {
                this.border_b_1().border_color(theme.title_bar_border)
            })
            .window_control_area(gpui::WindowControlArea::Drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(
                    |this, event: &MouseDownEvent, window, _| match event.click_count {
                        1 => this.grabbed = true,
                        2 if !SYSTEM_ZOOMS => window.zoom_window(),
                        _ => {}
                    },
                ),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.grabbed = false),
            )
            .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, _| this.grabbed = false))
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, window, _| {
                if this.grabbed {
                    this.grabbed = false;
                    window.start_window_move();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .when_else(
                        leading,
                        |this| this.pl_2(),
                        |this| this.pl(px(TITLE_BAR_LEFT_INSET)),
                    )
                    .pr_3()
                    .gap_1()
                    .when(offset > Pixels::ZERO, |this| this.w(offset))
                    .when(leading, |this| this.child(WindowControls::new(true)))
                    .when(navigation, |this| this.child(self.toggle(cx))),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .items_center()
                    .when(navigation, |this| this.child(self.history(cx)))
                    .children(content)
                    .pr_3(),
            )
            .when_some(
                self.options
                    .sidebar_right
                    .filter(|_| SidebarRight::available(window)),
                |this, open| {
                    this.child(div().flex_none().pr_3().child(self.lyrics_toggle(open, cx)))
                },
            )
            .when(decorated && !leading, |this| {
                this.child(div().flex_none().pr_2().child(WindowControls::new(false)))
            })
            .when(navigation && !on_search, |this| {
                let quick = self.quick_results(window, cx);
                this.child(self.search_bar(window, cx))
                    .when_some(quick, |this, popup| this.child(popup))
            })
    }
}

fn quick_hits(search: &Search) -> Vec<Hit> {
    QUICK_ORDER
        .into_iter()
        .flat_map(|(kind, room)| search.of(kind).take(room).cloned())
        .collect()
}

fn quick_heading(kind: Kind) -> SharedString {
    match kind {
        Kind::Artist => t!("search-artists"),
        Kind::Album => t!("search-quick-albums"),
        Kind::Song | Kind::Playlist => t!("search-songs"),
    }
}

fn quick_item(place: usize, hit: &Hit, playback: &Entity<Playback>) -> MenuItem {
    match hit {
        Hit::Song(track) => {
            let track = track.clone();
            let playback = playback.clone();
            MenuItem::new(("quick-hit", place), track.name.clone())
                .artwork(track.cover.clone())
                .detail(div().truncate().child(track.artists.clone()))
                .on_click(move |_, _, cx| {
                    playback.update(cx, |playback, cx| playback.play_radio(&track, cx));
                })
        }
        Hit::Artist(artist) => {
            let id = artist.id.clone();
            MenuItem::new(("quick-hit", place), artist.name.clone())
                .artwork(artist.cover.clone())
                .on_click(move |_, _, cx| {
                    if let Some(id) = id.clone() {
                        navigate(Destination::Artist(id.into()), cx);
                    }
                })
        }
        Hit::Album(album) => {
            let id = album.id.clone();
            MenuItem::new(("quick-hit", place), album.name.clone())
                .artwork(album.cover.clone())
                .detail(div().truncate().child(album.artists.clone()))
                .on_click(move |_, _, cx| navigate(Destination::Album(id.clone().into()), cx))
        }
        Hit::Playlist(list) => {
            let id = list.id.clone();
            MenuItem::new(("quick-hit", place), list.name.clone())
                .artwork(list.cover.clone())
                .on_click(move |_, _, cx| navigate(Destination::Playlist(id.clone().into()), cx))
        }
    }
}
