use gpui::prelude::*;
use gpui::{
    App, Context, Entity, FocusHandle, FontWeight, Global, Render, SharedString, Window, div,
};
use i18n::t;
use state::{Hit, Io, Kind, Outcome, Search, Sonora, Stock, Target, Toasts};
use ui::{ActiveTheme as _, Artwork, Button, Dismiss, FORM_CONTEXT, Input, Modal, Text};

// what a picked result is, and where it leads
struct Host {
    id: String,
    name: String,
    detail: Option<String>,
    cover: Option<String>,
}

struct Graftee {
    stock: Stock,
    id: String,
    title: String,
}

/// The dialog that grafts a local album onto an artist's releases, or a local
/// song onto an album's tracklist: a search field over the streaming catalog,
/// answered with artists or with albums, and a click places the item there.
pub(crate) struct Grafter {
    graftee: Option<Graftee>,
    input: Entity<Input>,
    search: Entity<Search>,
    focus: FocusHandle,
    restore: Option<FocusHandle>,
}

struct Installed(Entity<Grafter>);

impl Global for Installed {}

impl Grafter {
    pub fn entity(cx: &mut App) -> Entity<Self> {
        if cx.try_global::<Installed>().is_none() {
            let sonora = Sonora::global(cx);
            let session = sonora.session.clone();
            let library = sonora.library.clone();
            let grafter = cx.new(|cx| {
                let io = Io::global(cx);
                let search = cx.new(|cx| Search::new(session, library, io, cx));
                cx.observe(&search, |_, _, cx| cx.notify()).detach();
                let input = cx.new(|cx| Input::new("graft-search", cx).icon("icons/search.svg"));
                cx.observe(&input, |this: &mut Self, input, cx| {
                    let query = input.read(cx).text().to_owned();
                    this.search.update(cx, |search, cx| search.ask(&query, cx));
                })
                .detach();

                Self {
                    graftee: None,
                    input,
                    search,
                    focus: cx.focus_handle(),
                    restore: None,
                }
            });
            cx.set_global(Installed(grafter));
        }
        cx.global::<Installed>().0.clone()
    }

    pub fn open(
        stock: Stock,
        id: String,
        title: String,
        query: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let grafter = Self::entity(cx);
        grafter.update(cx, |this, cx| {
            this.restore = window.focused(cx);
            this.graftee = Some(Graftee { stock, id, title });
            this.input.update(cx, |input, cx| {
                input.set_text(query, cx);
                input.focus(window, cx);
            });
            cx.notify();
        });
    }

    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.graftee = None;
        self.input.update(cx, |input, cx| input.set_text("", cx));
        if let Some(focus) = self.restore.take() {
            window.focus(&focus, cx);
        }
        cx.notify();
    }

    fn pick(&mut self, host: &Host, window: &mut Window, cx: &mut Context<Self>) {
        let Some(graftee) = self.graftee.as_ref() else {
            return;
        };
        let (stock, id) = (graftee.stock, graftee.id.clone());
        let settings = Sonora::global(cx).settings.clone();
        settings.update(cx, |settings, cx| {
            settings.graft(stock, &host.id, &id, usize::MAX, cx)
        });
        let target = match stock {
            Stock::Discography => Target::Artist(host.id.clone().into()),
            Stock::Tracklist => Target::Album(host.id.clone().into()),
        };
        Toasts::linked(
            Outcome::Done,
            "toast-track-added",
            host.name.clone(),
            Some(target),
            cx,
        );
        self.close(window, cx);
    }

    fn hosts(&self, stock: Stock, cx: &App) -> Vec<Host> {
        let kind = match stock {
            Stock::Discography => Kind::Artist,
            Stock::Tracklist => Kind::Album,
        };
        self.search
            .read(cx)
            .of(kind)
            .filter_map(|hit| match hit {
                Hit::Artist(artist) => Some(Host {
                    id: artist.id.clone().filter(|id| !music::is_local_id(id))?,
                    name: artist.name.clone(),
                    detail: (artist.saved > 0)
                        .then(|| t!("graft-saved", count = artist.saved).to_string()),
                    cover: artist.cover.clone(),
                }),
                Hit::Album(album) if !music::is_local_id(&album.id) => Some(Host {
                    id: album.id.clone(),
                    name: album.name.clone(),
                    detail: Some(match album.year > 0 {
                        true => format!("{} · {}", album.artists, album.year),
                        false => album.artists.clone(),
                    }),
                    cover: album.cover.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn results(&self, stock: Stock, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *cx.theme();
        let hosts = self.hosts(stock, cx);
        let search = self.search.read(cx);
        let note = match (
            search.query().is_empty(),
            search.is_loading(),
            hosts.is_empty(),
        ) {
            (true, ..) => Some(t!("graft-hint")),
            (false, true, true) => Some(t!("graft-searching")),
            (false, false, true) => Some(t!("graft-nothing")),
            _ => None,
        };
        let art = theme.metrics.thumb;

        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(note.map(|note| {
                div()
                    .py_2()
                    .text_size(theme.text(Text::Small))
                    .text_color(theme.muted_foreground)
                    .child(note)
            }))
            .children(hosts.into_iter().enumerate().map(|(index, host)| {
                let cover = Artwork::new(host.cover.clone())
                    .size(art)
                    .when(stock == Stock::Discography, Artwork::circle);
                let name = SharedString::from(host.name.clone());
                let detail = host.detail.clone().map(SharedString::from);

                div()
                    .id(("graft-host", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_1()
                    .rounded(theme.radius)
                    .cursor_pointer()
                    .hover(|style| style.bg(theme.table_hover))
                    .child(cover)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .children(detail.map(|detail| {
                                div()
                                    .truncate()
                                    .text_size(theme.text(Text::Small))
                                    .text_color(theme.muted_foreground)
                                    .child(detail)
                            })),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.pick(&host, window, cx)))
            }))
    }
}

impl Render for Grafter {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(graftee) = self.graftee.as_ref() else {
            if let Some(focus) = self.restore.take() {
                window.focus(&focus, cx);
            }
            return div().into_any_element();
        };
        let theme = *cx.theme();
        let stock = graftee.stock;
        let title = match stock {
            Stock::Discography => t!("graft-discography-title"),
            Stock::Tracklist => t!("graft-album-title"),
        };
        let detail = SharedString::from(graftee.title.clone());

        div()
            .absolute()
            .inset_0()
            .key_context(FORM_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &Dismiss, window, cx| {
                cx.stop_propagation();
                this.close(window, cx);
            }))
            .child(
                Modal::new("grafter", title)
                    .w(theme.metrics.cover * 3.2)
                    .h(theme.metrics.cover * 3.6)
                    .detail(detail)
                    .child(self.input.clone())
                    .child(self.results(stock, cx))
                    .action(
                        Button::new("cancel-graft")
                            .ghost()
                            .label(t!("common-cancel"))
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .on_dismiss(cx.listener(|this, _, window, cx| this.close(window, cx))),
            )
            .into_any_element()
    }
}
