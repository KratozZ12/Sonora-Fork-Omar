use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

use anyhow::{Context as _, Result, anyhow, bail};
use async_trait::async_trait;
use serde::de::DeserializeOwned;

use super::wire;
use crate::{
    Album, AlbumDetail, Artist, ArtistProfile, MediaKind, MusicApi, Playlist, PlaylistDetail,
    SavedArtist, Track, UserProfile,
};

const BASE: &str = "https://samply.app/api/v0";
const AGENT: &str = concat!(
    "sonora/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/nolight132/sonora)"
);

async fn fetch<T: DeserializeOwned>(http: &reqwest::Client, token: &str, path: &str) -> Result<T> {
    let response = http
        .get(format!("{BASE}{path}"))
        .bearer_auth(token)
        .header(reqwest::header::USER_AGENT, AGENT)
        .send()
        .await
        .with_context(|| format!("cannot reach samply for {path}"))?;

    let status = response.status();
    if !status.is_success() {
        bail!("samply answered {status} for {path}");
    }
    response
        .json()
        .await
        .with_context(|| format!("cannot read the samply answer for {path}"))
}

pub struct SamplyClient {
    http: reqwest::Client,
    token: String,
    projects: RwLock<HashMap<String, wire::Project>>,
    /// Listing a project is slow — the listing itself takes seconds and every
    /// stack costs another request for its duration — and nothing in it changes
    /// while Sonora is open, so it is worked out once.
    listings: RwLock<HashMap<String, Vec<Track>>>,
}

impl SamplyClient {
    pub fn new(token: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            token,
            projects: RwLock::new(HashMap::new()),
            listings: RwLock::new(HashMap::new()),
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        fetch(&self.http, &self.token, path).await
    }

    pub async fn whoami(&self) -> Result<UserProfile> {
        let projects: Vec<wire::Project> = self.get("/projects").await?;
        let display_name = projects
            .iter()
            .find_map(|project| {
                project
                    .creator
                    .as_ref()
                    .and_then(|creator| creator.display_name.clone())
            })
            .unwrap_or_else(|| "Samply".to_owned());
        self.remember(projects);

        Ok(UserProfile {
            id: "samply".to_owned(),
            display_name,
        })
    }

    fn remember(&self, projects: Vec<wire::Project>) {
        let mut held = self.projects.write().unwrap();
        for project in projects {
            held.insert(project.id.clone(), project);
        }
    }

    async fn project(&self, id: &str) -> Result<wire::Project> {
        if let Some(project) = self.projects.read().unwrap().get(id) {
            return Ok(project.clone());
        }
        self.get(&format!("/projects/{id}")).await
    }

    /// Everything a project can play, in the order the project lists it.
    ///
    /// A project holds loose files and stacks, and a stack is one song with its
    /// takes inside it. The listing is flat: a stack's takes come back alongside
    /// it as files of their own, so they are matched up and dropped — they are
    /// drafts of the stack's song, not separate tracks — and the stack is listed
    /// under the name the account holder gave it. A stack carries no duration of
    /// its own, so it borrows the one of the take it plays, which is already in
    /// this same answer.
    async fn tracks_of(&self, project_id: &str) -> Result<Vec<Track>> {
        if let Some(listed) = self.listings.read().unwrap().get(project_id) {
            return Ok(listed.clone());
        }
        let project = self.project(project_id).await?;
        let boxes: Vec<wire::Box> = self.get(&format!("/projects/{project_id}/all")).await?;

        let takes: HashSet<&str> = boxes
            .iter()
            .flat_map(|entry| entry.children.iter())
            .map(|take| take.id.as_str())
            .collect();
        let files: HashMap<&str, &wire::Box> = boxes
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect();

        let mut listed: Vec<(&wire::Box, Option<&wire::Box>)> = Vec::new();
        for entry in &boxes {
            if let Some(take) = entry.latest_take() {
                listed.push((entry, files.get(take).copied()));
            } else if entry.playable() && !takes.contains(entry.id.as_str()) {
                listed.push((entry, None));
            }
        }
        wire::sort(&mut listed, project.sort_by.as_ref());

        let tracks: Vec<Track> = listed
            .iter()
            .enumerate()
            .map(|(place, (entry, played))| {
                let place = place as u32 + 1;
                match entry.latest_take() {
                    Some(take) => wire::stack_track(&project, entry, take, *played, place),
                    None => wire::file_track(&project, entry, place),
                }
            })
            .collect();

        self.listings
            .write()
            .unwrap()
            .insert(project_id.to_owned(), tracks.clone());
        Ok(tracks)
    }

    pub fn forget_listings(&self) {
        self.listings.write().unwrap().clear();
    }

    /// The signed url a file plays from. It expires, so it is asked for afresh
    /// rather than kept.
    pub async fn stream_url(&self, track_id: &str) -> Result<String> {
        let (project, file) = wire::parts_from_track_id(track_id)
            .ok_or_else(|| anyhow!("{track_id} is not a samply track id"))?;
        let link: wire::Download = self
            .get(&format!("/projects/{project}/files/{file}/download"))
            .await?;
        Ok(link.url)
    }

    pub async fn download(&self, track_id: &str) -> Result<bytes::Bytes> {
        let url = self.stream_url(track_id).await?;
        let response = self
            .http
            .get(url)
            .header(reqwest::header::USER_AGENT, AGENT)
            .send()
            .await
            .with_context(|| format!("cannot download {track_id}"))?;

        let status = response.status();
        if !status.is_success() {
            bail!("samply storage answered {status} for {track_id}");
        }
        response
            .bytes()
            .await
            .with_context(|| format!("cannot read the bytes of {track_id}"))
    }
}

#[async_trait]
impl MusicApi for SamplyClient {
    fn share_url(&self, kind: MediaKind, id: &str) -> Option<String> {
        let project = match kind {
            MediaKind::Album => wire::project_from_album_id(id)?.to_owned(),
            MediaKind::Track => wire::parts_from_track_id(id)?.0.to_owned(),
            MediaKind::Artist | MediaKind::Playlist => return None,
        };
        Some(format!("https://samply.app/project/{project}"))
    }

    async fn profile(&self) -> Result<UserProfile> {
        self.whoami().await
    }

    async fn saved_albums(&self, _limit: u32) -> Result<Vec<Album>> {
        let projects: Vec<wire::Project> = self.get("/projects").await?;
        self.forget_listings();
        let albums = projects.iter().map(wire::album).collect();
        self.remember(projects);
        Ok(albums)
    }

    async fn album(&self, album_id: &str) -> Result<AlbumDetail> {
        let id = wire::project_from_album_id(album_id)
            .ok_or_else(|| anyhow!("{album_id} is not a samply album id"))?;
        let project = self.project(id).await?;
        let tracks = self.tracks_of(id).await?;
        let mut album = wire::album(&project);
        album.track_count = tracks.len() as u32;

        Ok(AlbumDetail { album, tracks })
    }

    async fn album_tracks(&self, album_id: &str) -> Result<Vec<Track>> {
        let id = wire::project_from_album_id(album_id)
            .ok_or_else(|| anyhow!("{album_id} is not a samply album id"))?;
        self.tracks_of(id).await
    }

    async fn track(&self, track_id: &str) -> Result<Track> {
        let (project, file) = wire::parts_from_track_id(track_id)
            .ok_or_else(|| anyhow!("{track_id} is not a samply track id"))?;
        self.tracks_of(project)
            .await?
            .into_iter()
            .find(|track| track.id.as_deref() == Some(track_id))
            .ok_or_else(|| anyhow!("cannot find samply file {file}"))
    }

    async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let wanted = query.trim().to_lowercase();
        if wanted.is_empty() {
            return Ok(Vec::new());
        }
        let projects: Vec<wire::Project> = self.get("/projects").await?;
        self.remember(projects);
        let ids: Vec<String> = self.projects.read().unwrap().keys().cloned().collect();

        let mut found = Vec::new();
        for id in ids {
            let Ok(tracks) = self.tracks_of(&id).await else {
                continue;
            };
            found.extend(
                tracks
                    .into_iter()
                    .filter(|track| track.name.to_lowercase().contains(&wanted)),
            );
        }
        Ok(found)
    }

    async fn artist(&self, _artist_id: &str) -> Result<Artist> {
        bail!("samply has no artists")
    }

    async fn artist_profile(&self, _artist_id: &str) -> Result<ArtistProfile> {
        bail!("samply has no artists")
    }

    async fn artist_images(&self, _ids: Vec<String>) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }

    async fn saved_tracks(&self, _limit: u32) -> Result<Vec<Track>> {
        Ok(Vec::new())
    }

    async fn set_track_saved(&self, _track_id: &str, _saved: bool) -> Result<()> {
        bail!("samply files cannot be favourited")
    }

    async fn track_playcount(&self, _track_id: &str) -> Result<Option<u64>> {
        Ok(None)
    }

    async fn playlists(&self, _limit: u32) -> Result<Vec<Playlist>> {
        Ok(Vec::new())
    }

    async fn create_playlist(&self, _name: &str) -> Result<String> {
        bail!("samply has no playlists")
    }

    async fn rename_playlist(&self, _playlist_id: &str, _name: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn delete_playlist(&self, _playlist_id: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn remove_playlist_from_library(&self, _playlist_id: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn add_playlist_to_library(&self, _playlist_id: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn set_playlist_public(&self, _playlist_id: &str, _public: bool) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn add_track_to_playlist(&self, _playlist_id: &str, _track_id: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn remove_track_from_playlist(&self, _playlist_id: &str, _track_id: &str) -> Result<()> {
        bail!("samply has no playlists")
    }

    async fn set_album_saved(&self, _album_id: &str, _saved: bool) -> Result<()> {
        bail!("samply projects are always yours")
    }

    async fn saved_artists(&self, _limit: u32) -> Result<Vec<SavedArtist>> {
        Ok(Vec::new())
    }

    async fn set_artist_saved(&self, _artist_id: &str, _saved: bool) -> Result<()> {
        bail!("samply has no artists")
    }

    async fn playlist(&self, _playlist_id: &str) -> Result<PlaylistDetail> {
        bail!("samply has no playlists")
    }

    async fn playlist_tracks(&self, _playlist_id: &str) -> Result<Vec<Track>> {
        Ok(Vec::new())
    }

    async fn playlist_covers(&self, _playlist_id: &str, _wanted: usize) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn track_radio(&self, _track_id: &str) -> Result<Vec<Track>> {
        Ok(Vec::new())
    }
}
