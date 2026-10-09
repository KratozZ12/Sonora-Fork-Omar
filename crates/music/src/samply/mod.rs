pub(crate) mod client;
mod playback;
mod wire;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;

use crate::{InputSource, MusicProvider, PlaybackFactory, PromptSink, ProviderSession, SignIn};

pub const SAMPLY_TRACK_PREFIX: &str = "samply:";
pub const SAMPLY_ALBUM_PREFIX: &str = "samply-album:";

const TOKEN_FILE: &str = "samply-token";
const TOKEN_ENV: &str = "SAMPLY_TOKEN";

pub struct SamplyProvider {
    state_dir: PathBuf,
}

impl SamplyProvider {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir }
    }

    fn cache_dir(&self) -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("sonora")
            .join("samply-files")
    }

    /// The token is never written by Sonora: it is read from the environment or
    /// from a file the account holder puts there themselves.
    fn token(&self) -> Option<String> {
        if let Ok(token) = std::env::var(TOKEN_ENV)
            && !token.trim().is_empty()
        {
            return Some(token.trim().to_owned());
        }
        std::fs::read_to_string(self.state_dir.join(TOKEN_FILE))
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty())
    }

    async fn open(&self, token: String) -> Result<ProviderSession> {
        let client = Arc::new(client::SamplyClient::new(token));
        let profile = client.whoami().await?;
        let playback: Arc<dyn PlaybackFactory> =
            Arc::new(playback::Factory::new(client.clone(), self.cache_dir()));

        Ok(ProviderSession {
            profile,
            api: client,
            playback,
            authenticated: true,
            playcounts: false,
        })
    }
}

#[async_trait]
impl MusicProvider for SamplyProvider {
    fn name(&self) -> &'static str {
        "Samply"
    }

    fn slug(&self) -> &'static str {
        "samply"
    }

    fn sign_in_options(&self) -> Vec<SignIn> {
        vec![SignIn::Secret]
    }

    fn stored(&self) -> bool {
        self.token().is_some()
    }

    async fn restore(&self) -> Result<Option<ProviderSession>> {
        let Some(token) = self.token() else {
            return Ok(None);
        };
        self.open(token).await.map(Some)
    }

    async fn sign_in(
        &self,
        _method: SignIn,
        _prompt: PromptSink,
        _input: InputSource,
    ) -> Result<ProviderSession> {
        let token = self.token().ok_or_else(|| {
            anyhow!(
                "cannot find a samply token: put one in {} or set {TOKEN_ENV}",
                self.state_dir.join(TOKEN_FILE).display()
            )
        })?;
        self.open(token).await
    }

    fn sign_out(&self) {}
}

pub fn is_samply_id(id: &str) -> bool {
    id.starts_with(SAMPLY_TRACK_PREFIX) || id.starts_with(SAMPLY_ALBUM_PREFIX)
}
