use anyhow::{Result, bail};

use crate::MusicApi;
use crate::samply::client::SamplyClient;

const TOKEN_ENV: &str = "SAMPLY_TOKEN";
const TOKEN_FILE: &str = "sonora/samply-token";

#[tokio::test]
#[ignore = "reads the projects of the connected Samply account"]
async fn samply_projects_arrive_as_albums_with_playable_files() -> Result<()> {
    let Some(token) = token() else {
        bail!("no samply token: set {TOKEN_ENV} or write ~/.config/{TOKEN_FILE}");
    };
    let api = SamplyClient::new(token);

    let profile = api.profile().await?;
    println!("signed in as {}", profile.display_name);

    let albums = api.saved_albums(0).await?;
    println!("projects: {}", albums.len());
    if albums.is_empty() {
        bail!("the account has no projects to check");
    }
    for album in albums.iter().take(5) {
        println!(
            "  {:?} cover={} id={}",
            album.name,
            album.cover.is_some(),
            album.id
        );
    }

    let first = &albums[0];
    let tracks = api.album_tracks(&first.id).await?;
    println!("files in {:?}: {}", first.name, tracks.len());
    if tracks.is_empty() {
        bail!("{:?} holds no playable files", first.name);
    }
    for track in tracks.iter().take(5) {
        println!(
            "  {:?} by {:?} {:?}",
            track.name, track.artists, track.duration
        );
    }

    let track = &tracks[0];
    let id = track.id.as_deref().expect("a samply track carries an id");
    let fetched = api.track(id).await?;
    assert_eq!(fetched.name, track.name);

    let url = api.stream_url(id).await?;
    if !url.starts_with("https://") {
        bail!("the download url is not https");
    }
    println!("playable url resolved, {} bytes of signature", url.len());

    let bytes = api.download(id).await?;
    println!("downloaded {} KiB of {:?}", bytes.len() / 1024, track.name);
    if bytes.len() < 1024 {
        bail!("the downloaded file is too small to be audio");
    }

    let decoded = rodio::Decoder::builder()
        .with_data(std::io::Cursor::new(bytes.to_vec()))
        .with_seekable(true)
        .with_byte_len(bytes.len() as u64)
        .build();
    match decoded {
        Ok(source) => {
            use rodio::Source as _;
            println!(
                "decodes: {} Hz, {} channels, {:?}",
                source.sample_rate(),
                source.channels(),
                source.total_duration()
            );
        }
        Err(error) => bail!("the downloaded file does not decode: {error}"),
    }

    Ok(())
}

fn token() -> Option<String> {
    if let Ok(token) = std::env::var(TOKEN_ENV)
        && !token.trim().is_empty()
    {
        return Some(token.trim().to_owned());
    }
    let path = dirs::config_dir()?.join(TOKEN_FILE);
    std::fs::read_to_string(path)
        .ok()
        .map(|token| token.trim().to_owned())
        .filter(|token| !token.is_empty())
}
