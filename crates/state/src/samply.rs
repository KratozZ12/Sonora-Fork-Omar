use std::rc::Rc;

use gpui::{Context, Entity, Task};
use music::Album;

use crate::{Io, Session, SessionEvent, join};

/// The projects of the connected Samply account, listed as albums.
pub struct Samply {
    projects: Rc<Vec<Album>>,
    loading: bool,
    error: Option<String>,
    session: Entity<Session>,
    io: Io,
    task: Option<Task<()>>,
}

impl Samply {
    pub fn new(session: Entity<Session>, io: Io, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&session, |this, _, event, cx| match event {
            SessionEvent::LocalChanged => this.load(cx),
            SessionEvent::SignedIn | SessionEvent::SignedOut | SessionEvent::Reconnected => {}
        })
        .detach();

        Self {
            projects: Rc::new(Vec::new()),
            loading: false,
            error: None,
            session,
            io,
            task: None,
        }
    }

    pub fn projects(&self) -> Rc<Vec<Album>> {
        self.projects.clone()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn connected(&self, cx: &gpui::App) -> bool {
        self.session.read(cx).samply_ready()
    }

    pub fn load(&mut self, cx: &mut Context<Self>) {
        if self.loading {
            return;
        }
        let Some(client) = self.session.read(cx).samply_client() else {
            return;
        };

        self.loading = true;
        self.error = None;
        cx.notify();

        let io = self.io.clone();
        self.task = Some(cx.spawn(async move |this, cx| {
            let loaded = join(io.spawn(async move { client.saved_albums(0).await })).await;

            this.update(cx, |this, cx| {
                this.loading = false;
                this.task = None;
                match loaded {
                    Ok(projects) => {
                        this.projects = Rc::new(projects);
                        this.error = None;
                    }
                    Err(error) => {
                        log::warn!("samply: cannot list the projects: {error:#}");
                        this.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.task = None;
        self.loading = false;
        self.load(cx);
    }
}
