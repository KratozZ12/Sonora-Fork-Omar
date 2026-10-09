use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use super::client::SamplyClient;
use super::wire;
use crate::spectrum::Spectrum;
use crate::{
    LOCAL_TRACK_PREFIX, PlaybackConfig, PlaybackEvent, PlaybackEvents, PlaybackFactory, Player,
    local,
};

/// How much downloaded audio is kept before the files played longest ago are
/// dropped.
const BUDGET: u64 = 2 * 1024 * 1024 * 1024;

enum Command {
    Load {
        id: String,
        at: Option<Duration>,
        seamless: bool,
    },
    Preload {
        id: String,
    },
}

/// Plays Samply files through the engine the local library already uses.
///
/// A file is fetched to the cache first and handed over as an ordinary path, so
/// seeking, gapless and the spectrum all behave the way they do for a file on
/// disk. Signed urls expire, and a part-downloaded stream cannot be seeked, so
/// the whole file is taken before playback starts rather than streamed.
pub struct Factory {
    client: Arc<SamplyClient>,
    cache: PathBuf,
}

impl Factory {
    pub fn new(client: Arc<SamplyClient>, cache: PathBuf) -> Self {
        Self { client, cache }
    }
}

impl PlaybackFactory for Factory {
    fn start(&self, config: PlaybackConfig) -> (Box<dyn Player>, Box<dyn PlaybackEvents>) {
        let (inner_player, inner_events) = local::playback::Factory.start(config);
        let inner: Arc<dyn Player> = Arc::from(inner_player);
        let (commands, command_rx) = unbounded_channel();
        let (events, event_rx) = unbounded_channel();

        let client = self.client.clone();
        let cache = self.cache.clone();
        let fetching = inner.clone();
        let spawned = std::thread::Builder::new()
            .name("samply-playback".to_owned())
            .spawn(move || run(client, cache, fetching, command_rx, events, inner_events));
        if let Err(error) = spawned {
            log::error!("playback: cannot spawn samply engine thread: {error}");
        }

        (
            Box::new(Engine { inner, commands }),
            Box::new(Events(event_rx)),
        )
    }
}

struct Engine {
    inner: Arc<dyn Player>,
    commands: UnboundedSender<Command>,
}

impl Player for Engine {
    fn load(&self, track_id: &str, seamless: bool) -> Result<()> {
        self.commands
            .send(Command::Load {
                id: track_id.to_owned(),
                at: None,
                seamless,
            })
            .context("cannot reach samply playback engine")
    }

    fn load_paused_at(&self, track_id: &str, at: Duration) -> Result<()> {
        self.commands
            .send(Command::Load {
                id: track_id.to_owned(),
                at: Some(at),
                seamless: false,
            })
            .context("cannot reach samply playback engine")
    }

    fn preload(&self, track_id: &str) -> Result<()> {
        self.commands
            .send(Command::Preload {
                id: track_id.to_owned(),
            })
            .context("cannot reach samply playback engine")
    }

    fn play(&self) {
        self.inner.play();
    }

    fn pause(&self) {
        self.inner.pause();
    }

    fn seek(&self, position: Duration) {
        self.inner.seek(position);
    }

    fn set_gain(&self, gain: f32) {
        self.inner.set_gain(gain);
    }

    fn spectrum(&self) -> Option<Spectrum> {
        self.inner.spectrum()
    }
}

struct Events(UnboundedReceiver<PlaybackEvent>);

#[async_trait]
impl PlaybackEvents for Events {
    async fn next(&mut self) -> Option<PlaybackEvent> {
        self.0.recv().await
    }
}

fn run(
    client: Arc<SamplyClient>,
    cache: PathBuf,
    inner: Arc<dyn Player>,
    commands: UnboundedReceiver<Command>,
    events: UnboundedSender<PlaybackEvent>,
    inner_events: Box<dyn PlaybackEvents>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            log::error!("playback: cannot build samply engine runtime: {error}");
            return;
        }
    };
    runtime.block_on(engine_loop(
        client,
        cache,
        inner,
        commands,
        events,
        inner_events,
    ));
}

async fn engine_loop(
    client: Arc<SamplyClient>,
    cache: PathBuf,
    inner: Arc<dyn Player>,
    mut commands: UnboundedReceiver<Command>,
    events: UnboundedSender<PlaybackEvent>,
    mut inner_events: Box<dyn PlaybackEvents>,
) {
    let relay = events.clone();
    let forwarding = async move {
        while let Some(event) = inner_events.next().await {
            if relay.send(event).is_err() {
                break;
            }
        }
    };

    let working = async move {
        while let Some(command) = commands.recv().await {
            match command {
                Command::Load { id, at, seamless } => {
                    events
                        .send(PlaybackEvent::Loading(at.unwrap_or_default()))
                        .ok();
                    let held = match fetch(&client, &cache, &id).await {
                        Ok(path) => path,
                        Err(error) => {
                            log::warn!("playback: cannot fetch {id}: {error:#}");
                            events.send(PlaybackEvent::Unavailable).ok();
                            continue;
                        }
                    };
                    let local = local_id(&held);
                    let sent = match at {
                        Some(at) => inner.load_paused_at(&local, at),
                        None => inner.load(&local, seamless),
                    };
                    if let Err(error) = sent {
                        log::warn!("playback: cannot play {id}: {error:#}");
                        events.send(PlaybackEvent::Unavailable).ok();
                    }
                }
                Command::Preload { id } => match fetch(&client, &cache, &id).await {
                    Ok(held) => {
                        if let Err(error) = inner.preload(&local_id(&held)) {
                            log::warn!("playback: cannot preload {id}: {error:#}");
                        }
                    }
                    Err(error) => log::warn!("playback: cannot fetch {id}: {error:#}"),
                },
            }
        }
    };

    tokio::join!(forwarding, working);
}

fn local_id(path: &Path) -> String {
    format!("{LOCAL_TRACK_PREFIX}{}", path.display())
}

/// Brings a file down once and keeps it, so replaying it costs nothing and a
/// url that has since expired is never needed again.
async fn fetch(client: &SamplyClient, cache: &Path, track_id: &str) -> Result<PathBuf> {
    let (_, file) = wire::parts_from_track_id(track_id)
        .ok_or_else(|| anyhow::anyhow!("{track_id} is not a samply track id"))?;
    let held = cache.join(file);
    if held.is_file() {
        touch(&held);
        return Ok(held);
    }

    std::fs::create_dir_all(cache).with_context(|| format!("cannot create {}", cache.display()))?;
    let bytes = client.download(track_id).await?;

    let partial = held.with_extension("part");
    std::fs::write(&partial, &bytes)
        .with_context(|| format!("cannot write {}", partial.display()))?;
    std::fs::rename(&partial, &held).with_context(|| format!("cannot place {}", held.display()))?;
    trim(cache, &held);
    Ok(held)
}

/// Marks a file as played now, so replaying it keeps it out of the way of
/// [`trim`].
fn touch(held: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(held) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Drops the files played longest ago until the cache is back under budget. The
/// one just fetched is kept whatever its size, so a file larger than the whole
/// budget still plays.
fn trim(cache: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    let mut held: Vec<(SystemTime, u64, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let meta = entry.metadata().ok()?;
            match meta.is_file() {
                true => Some((meta.modified().ok()?, meta.len(), entry.path())),
                false => None,
            }
        })
        .collect();

    let mut total: u64 = held.iter().map(|(_, size, _)| size).sum();
    if total <= BUDGET {
        return;
    }

    held.sort_by_key(|(played, _, _)| *played);
    for (_, size, path) in held {
        if total <= BUDGET {
            break;
        }
        if path == keep {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                log::debug!("playback: dropped {} from the samply cache", path.display());
                total = total.saturating_sub(size);
            }
            Err(error) => log::warn!("playback: cannot drop {}: {error}", path.display()),
        }
    }
}
