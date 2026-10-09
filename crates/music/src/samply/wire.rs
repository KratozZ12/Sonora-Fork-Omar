use std::cmp::Ordering;
use std::time::Duration;

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use serde::Deserialize;

use super::{SAMPLY_ALBUM_PREFIX, SAMPLY_TRACK_PREFIX};
use crate::{Album, ArtistRef, ReleaseType, Track};

const SEPARATOR: &str = " - ";

/// Samply stores a file under the name it was uploaded with and hands the
/// storage url back with that name in it verbatim, spaces and all, so the url
/// has to be escaped before anything can fetch it. `%` is deliberately left
/// alone: a url that already carries an escape must not be escaped twice.
const IN_PATH: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'`')
    .add(b'#')
    .add(b'?')
    .add(b'{')
    .add(b'}');

#[derive(Deserialize, Clone)]
pub struct Project {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub artwork: Option<String>,
    #[serde(default)]
    pub creator: Option<Creator>,
    #[serde(default, rename = "sortBy")]
    pub sort_by: Option<SortBy>,
}

#[derive(Deserialize, Clone)]
pub struct Creator {
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct SortBy {
    #[serde(default)]
    pub criterion: String,
    #[serde(default = "ascending")]
    pub ascending: bool,
}

fn ascending() -> bool {
    true
}

#[derive(Deserialize)]
pub struct Box {
    pub id: String,
    #[serde(default)]
    pub object: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub trashed: Option<bool>,
    #[serde(default, rename = "timeCreated")]
    pub time_created: Option<f64>,
    /// A stack is one song with its takes in it; a plain file has none.
    #[serde(default)]
    pub children: Vec<Child>,
}

#[derive(Deserialize)]
pub struct Child {
    pub id: String,
}

#[derive(Deserialize)]
pub struct Download {
    pub url: String,
}

impl Box {
    pub fn kept(&self) -> bool {
        self.trashed != Some(true)
    }

    pub fn playable(&self) -> bool {
        self.object == "file" && self.duration.is_some() && self.kept()
    }

    /// The take a stack plays: the newest one it holds.
    pub fn latest_take(&self) -> Option<&str> {
        match self.object == "stack" && self.kept() {
            true => self.children.last().map(|child| child.id.as_str()),
            false => None,
        }
    }
}

pub fn album_id(project: &str) -> String {
    format!("{SAMPLY_ALBUM_PREFIX}{project}")
}

pub fn project_from_album_id(id: &str) -> Option<&str> {
    id.strip_prefix(SAMPLY_ALBUM_PREFIX)
}

pub fn track_id(project: &str, file: &str) -> String {
    format!("{SAMPLY_TRACK_PREFIX}{project}/{file}")
}

pub fn parts_from_track_id(id: &str) -> Option<(&str, &str)> {
    id.strip_prefix(SAMPLY_TRACK_PREFIX)?.split_once('/')
}

pub fn album(project: &Project) -> Album {
    let artist = owner(project);
    let cover = artwork(project);
    Album {
        id: album_id(&project.id),
        name: project.name.clone(),
        artists: artist.clone(),
        artist_refs: vec![artist_ref(&artist)],
        cover: cover.clone(),
        cover_large: cover,
        release_type: ReleaseType::Album,
        year: 0,
        track_count: 0,
        release_date: String::new(),
        label: String::new(),
        copyrights: Vec::new(),
        added_at: None,
    }
}

/// The order a project lists its files in.
///
/// Samply keeps a hand-arranged order in its own app and its api does not hand
/// it back, so `custom` — and every criterion this listing cannot answer — is
/// served by the order the files were added, which is the closest the api gets.
/// The sort is stable, so anything the criterion ties on keeps the order it
/// arrived in.
pub fn sort(listed: &mut [(&Box, Option<&Box>)], by: Option<&SortBy>) {
    match by.map(|by| by.criterion.as_str()) {
        Some("name") => listed.sort_by_key(|one| one.0.name.to_lowercase()),
        Some("duration") => listed.sort_by(|one, two| number(length(one), length(two))),
        _ => listed.sort_by(|one, two| number(one.0.time_created, two.0.time_created)),
    }
    if !by.map(|by| by.ascending).unwrap_or(true) {
        listed.reverse();
    }
}

fn length(entry: &(&Box, Option<&Box>)) -> Option<f64> {
    match entry.1 {
        Some(played) => played.duration,
        None => entry.0.duration,
    }
}

fn number(one: Option<f64>, two: Option<f64>) -> Ordering {
    one.unwrap_or_default()
        .partial_cmp(&two.unwrap_or_default())
        .unwrap_or(Ordering::Equal)
}

/// A loose file: nobody named it in Samply, so the name it was uploaded with is
/// all there is to go on.
pub fn file_track(project: &Project, box_: &Box, place: u32) -> Track {
    let (artist, name) = split(&box_.name, &owner(project));
    build(project, artist, name, &box_.id, box_.duration, place)
}

/// A stack: the name is the one the account holder typed in Samply, so it is
/// kept exactly as it stands — no extension to strip, nothing to read out of it.
/// Only the artist still comes from the take's filename, because a stack holds
/// the song's name and nothing else.
pub fn stack_track(
    project: &Project,
    stack: &Box,
    take: &str,
    played: Option<&Box>,
    place: u32,
) -> Track {
    let title = match stack.name.trim() {
        "" => take,
        trimmed => trimmed,
    };
    let artist = match played {
        Some(played) => split(&played.name, &owner(project)).0,
        None => owner(project),
    };
    build(
        project,
        artist,
        title.to_owned(),
        take,
        played.and_then(|played| played.duration),
        place,
    )
}

fn build(
    project: &Project,
    artist: String,
    name: String,
    file: &str,
    duration: Option<f64>,
    place: u32,
) -> Track {
    Track {
        id: Some(track_id(&project.id, file)),
        name,
        playable: true,
        artists: artist.clone(),
        artist_refs: vec![artist_ref(&artist)],
        album: project.name.clone(),
        album_id: Some(album_id(&project.id)),
        cover: artwork(project),
        duration: Duration::from_secs_f64(duration.unwrap_or_default().max(0.)),
        added_at: None,
        added_by: None,
        playcount: None,
        popularity: 0,
        explicit: false,
        track_number: place,
        disc_number: 0,
        tags: Vec::new(),
        languages: Vec::new(),
        credits: Vec::new(),
    }
}

fn artwork(project: &Project) -> Option<String> {
    project.artwork.as_deref().map(escaped)
}

/// Escapes the path of a url and leaves the scheme and host as they are.
fn escaped(url: &str) -> String {
    let cut = url
        .find("://")
        .and_then(|scheme| url[scheme + 3..].find('/').map(|path| scheme + 3 + path))
        .unwrap_or(url.len());
    let (head, path) = url.split_at(cut);
    format!("{head}{}", utf8_percent_encode(path, IN_PATH))
}

fn owner(project: &Project) -> String {
    project
        .creator
        .as_ref()
        .and_then(|creator| creator.display_name.clone())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Samply".to_owned())
}

fn artist_ref(name: &str) -> ArtistRef {
    ArtistRef {
        name: name.to_owned(),
        id: None,
    }
}

/// Samply files carry a filename and nothing else, so the artist is read out of
/// it the way it is usually written, and falls back to whoever owns the project.
fn split(filename: &str, fallback: &str) -> (String, String) {
    let stem = filename
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(filename)
        .trim();
    match stem.split_once(SEPARATOR) {
        Some((artist, title)) if !artist.trim().is_empty() && !title.trim().is_empty() => {
            (artist.trim().to_owned(), title.trim().to_owned())
        }
        _ => (fallback.to_owned(), stem.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_artist_out_of_a_filename_that_carries_one() {
        let (artist, title) = split("Kanye West - No Child Left Behind.flac", "me");
        assert_eq!(artist, "Kanye West");
        assert_eq!(title, "No Child Left Behind");
    }

    #[test]
    fn a_bare_filename_keeps_the_project_owner() {
        let (artist, title) = split("NEW_AGAIN.mp3", "me");
        assert_eq!(artist, "me");
        assert_eq!(title, "NEW_AGAIN");
    }

    #[test]
    fn a_track_id_survives_a_round_trip() {
        let id = track_id("proj", "file");
        assert_eq!(parts_from_track_id(&id), Some(("proj", "file")));
    }
}
