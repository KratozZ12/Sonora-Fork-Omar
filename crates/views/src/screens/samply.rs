use gpui::prelude::*;
use gpui::{Context, Entity, Render, ScrollHandle, Window, div};
use i18n::t;
use state::{Playback, Samply};
use ui::{ActiveTheme as _, Button, Scrollbar, Scroller, Text, Vacancy, heading};

use crate::chrome::Chrome;
use crate::shared::album_grid::AlbumGrid;
use crate::shared::cells;

pub(crate) struct SamplyView {
    projects: Entity<Samply>,
    playback: Entity<Playback>,
    scrollbar: Entity<Scrollbar>,
}

impl SamplyView {
    pub(crate) fn new(
        projects: Entity<Samply>,
        playback: Entity<Playback>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&projects, |_, _, cx| cx.notify()).detach();
        let chrome = Chrome::entity(cx);
        cx.observe(&chrome, |_, _, cx| cx.notify()).detach();
        let me = cx.entity_id();

        Self {
            projects,
            playback,
            scrollbar: cx.new(|_| Scrollbar::new(ScrollHandle::new()).watching(me)),
        }
    }

    pub(crate) fn refresh(&self, cx: &mut Context<Self>) {
        self.projects.update(cx, |projects, cx| projects.load(cx));
    }

    fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.projects.read(cx).is_loading();

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_2()
            .child(heading(t!("nav-samply"), cx))
            .child(
                Button::new("refresh-samply")
                    .ghost()
                    .small()
                    .icon("icons/rotate-ccw-clock.svg")
                    .tooltip("samply-refresh")
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.projects
                            .update(cx, |projects, cx| projects.refresh(cx));
                    })),
            )
    }
}

impl Render for SamplyView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let pad = theme.metrics.inset;
        let width = cells::content_width(window, pad * 2., cx);
        let projects = self.projects.read(cx);
        let connected = projects.connected(cx);
        let loading = projects.is_loading();
        let error = projects.error().map(str::to_owned);
        let held = projects.projects();

        let body = match (connected, held.is_empty(), loading) {
            (false, _, _) => Vacancy::new(t!("samply-unconfigured"))
                .icon("icons/link.svg")
                .flex_1()
                .into_any_element(),
            (true, true, true) => Vacancy::new(t!("samply-loading"))
                .icon("icons/rotate-ccw-clock.svg")
                .flex_1()
                .into_any_element(),
            (true, true, false) => Vacancy::new(t!("samply-empty"))
                .icon("icons/file-music.svg")
                .flex_1()
                .into_any_element(),
            (true, false, _) => Scroller::new("samply-projects", &self.scrollbar)
                .pb(pad)
                .child(
                    AlbumGrid::new(
                        "samply-project",
                        width,
                        held.iter().cloned().enumerate(),
                        self.playback.clone(),
                    )
                    .into_any_element(),
                )
                .into_any_element(),
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .gap_4()
            .p(pad)
            .child(self.header(cx))
            .children(error.map(|error| {
                div()
                    .flex_none()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.danger)
                    .child(error)
            }))
            .child(body)
    }
}
