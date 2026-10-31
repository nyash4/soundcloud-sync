//! SoundCloud metadata adapter (PROTOCOL §6): official API with credentials, oEmbed without.

mod api;
mod oembed;
pub mod url;

use std::time::Duration;

use ::url::Url;
use reqwest::{StatusCode, header};

use self::url::Link;
use crate::config::SoundCloudCredentials;
use crate::protocol::{Collection, TrackInfo, Unavailable};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_SHORT_LINK_REDIRECTS: usize = 5;
const USER_AGENT: &str = concat!("scsync/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("not an allowed SoundCloud link")]
    BadUrl,
    #[error("SoundCloud does not know this link")]
    NotFound,
    #[error("link points to something that is not a track or collection")]
    Unsupported,
    #[error("collection exceeds the import limit")]
    TooLarge,
    #[error("SoundCloud request failed: {0}")]
    Upstream(String),
}

impl From<reqwest::Error> for ImportError {
    fn from(e: reqwest::Error) -> Self {
        ImportError::Upstream(e.without_url().to_string())
    }
}

/// Maps a non-success upstream status to an import error.
fn status_error(status: StatusCode) -> ImportError {
    match status {
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => {
            ImportError::NotFound
        }
        other => ImportError::Upstream(format!("HTTP {other}")),
    }
}

#[derive(Debug)]
pub enum Resolved {
    Tracks {
        collection: Option<Collection>,
        tracks: Vec<TrackInfo>,
        unavailable: Vec<Unavailable>,
        missing: usize,
    },
    /// oEmbed mode cannot list a collection; the client expands it with the widget (§6).
    NeedsExpansion {
        collection: Collection,
        playlist_url: String,
    },
}

pub struct SoundCloud {
    /// Never follows redirects on its own: short links are expanded hop by hop and checked.
    http: reqwest::Client,
    api: Option<api::Api>,
    max_tracks: usize,
}

impl SoundCloud {
    pub fn new(
        credentials: Option<SoundCloudCredentials>,
        max_tracks: usize,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let api = credentials.map(api::Api::new).transpose()?;
        Ok(Self {
            http,
            api,
            max_tracks,
        })
    }

    pub fn has_api(&self) -> bool {
        self.api.is_some()
    }

    pub fn max_tracks(&self) -> usize {
        self.max_tracks
    }

    /// Resolves a user-supplied link. `track_ids` is the client-expanded collection in oEmbed mode.
    pub async fn resolve(
        &self,
        input: &str,
        track_ids: Option<&[u64]>,
    ) -> Result<Resolved, ImportError> {
        let link = match url::parse(input)? {
            Link::Short(short) => self.expand_short(short).await?,
            link => link,
        };
        match &self.api {
            Some(api) => api.resolve(&link, self.max_tracks).await,
            None => oembed::resolve(&self.http, &link, track_ids, self.max_tracks).await,
        }
    }

    /// Follows `on.soundcloud.com` redirects manually, accepting only SoundCloud targets.
    async fn expand_short(&self, mut current: Url) -> Result<Link, ImportError> {
        for _ in 0..MAX_SHORT_LINK_REDIRECTS {
            let response = self.http.get(current.clone()).send().await?;
            let status = response.status();
            if !status.is_redirection() {
                return Err(match status {
                    StatusCode::NOT_FOUND => ImportError::NotFound,
                    s if s.is_success() => ImportError::Unsupported,
                    s => status_error(s),
                });
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| current.join(v).ok())
                .ok_or_else(|| ImportError::Upstream("redirect without location".into()))?;
            match url::parse(location.as_str())? {
                Link::Short(next) => current = next,
                link => return Ok(link),
            }
        }
        Err(ImportError::BadUrl)
    }
}
