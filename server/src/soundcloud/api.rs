//! Official SoundCloud API (client-credentials OAuth).

use std::time::{Duration, Instant};

use reqwest::{StatusCode, header};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::Mutex;

use super::url::{self, Kind, Link};
use super::{ImportError, REQUEST_TIMEOUT, Resolved, USER_AGENT, status_error};
use crate::config::SoundCloudCredentials;
use crate::protocol::{Collection, CollectionKind, TrackInfo, Unavailable, UnavailableReason};

const TOKEN_URL: &str = "https://secure.soundcloud.com/oauth/token";
const API_BASE: &str = "https://api.soundcloud.com";
const API_HOST: &str = "api.soundcloud.com";
const PAGE_SIZE: usize = 200;
/// Refresh the access token this long before SoundCloud says it expires.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(120);

pub struct Api {
    http: reqwest::Client,
    credentials: SoundCloudCredentials,
    token: Mutex<Option<(String, Instant)>>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct User {
    username: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Deserialize)]
struct Track {
    id: u64,
    title: Option<String>,
    user: Option<User>,
    duration: Option<u64>,
    artwork_url: Option<String>,
    permalink_url: Option<String>,
    embeddable_by: Option<String>,
    access: Option<String>,
    streamable: Option<bool>,
    secret_token: Option<String>,
}

#[derive(Deserialize)]
struct Playlist {
    id: u64,
    title: Option<String>,
    playlist_type: Option<String>,
    set_type: Option<String>,
    track_count: Option<usize>,
    secret_token: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    #[serde(default)]
    collection: Vec<Value>,
    next_href: Option<String>,
}

impl Track {
    fn unavailable_reason(&self) -> Option<UnavailableReason> {
        if self.embeddable_by.as_deref().is_some_and(|e| e != "all") {
            return Some(UnavailableReason::NotEmbeddable);
        }
        match self.access.as_deref() {
            Some("blocked") => return Some(UnavailableReason::Blocked),
            Some("preview") => return Some(UnavailableReason::PreviewOnly),
            _ => {}
        }
        (self.streamable == Some(false)).then_some(UnavailableReason::NotStreamable)
    }

    fn into_info(self, secret_token: Option<&str>) -> TrackInfo {
        let secret = self.secret_token.as_deref().or(secret_token);
        let widget_url = url::api_url(Kind::Track, self.id, secret);
        let artwork = self
            .artwork_url
            .or_else(|| self.user.as_ref().and_then(|u| u.avatar_url.clone()))
            .filter(|u| u.starts_with("https://"))
            .map(|u| u.replace("-large", "-t500x500"));
        TrackInfo {
            track_id: self.id,
            permalink: self
                .permalink_url
                .unwrap_or_else(|| url::player_link(&widget_url)),
            widget_url,
            title: self.title.unwrap_or_default(),
            artist: self.user.and_then(|u| u.username).unwrap_or_default(),
            duration_ms: self.duration,
            artwork,
        }
    }
}

fn collection_kind(playlist: &Playlist) -> CollectionKind {
    let is_album = [&playlist.playlist_type, &playlist.set_type]
        .into_iter()
        .flatten()
        .any(|t| matches!(t.as_str(), "album" | "ep" | "single" | "compilation"));
    if is_album {
        CollectionKind::Album
    } else {
        CollectionKind::Playlist
    }
}

impl Api {
    pub fn new(credentials: SoundCloudCredentials) -> anyhow::Result<Self> {
        // `/resolve` answers with a redirect to the resource; only follow those within the API host.
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || attempt.url().host_str() != Some(API_HOST) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        });
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(REQUEST_TIMEOUT)
            .redirect(policy)
            .build()?;
        Ok(Self {
            http,
            credentials,
            token: Mutex::new(None),
        })
    }

    async fn token(&self, force_refresh: bool) -> Result<String, ImportError> {
        let mut cached = self.token.lock().await;
        if !force_refresh
            && let Some((token, valid_until)) = cached.as_ref()
            && Instant::now() < *valid_until
        {
            return Ok(token.clone());
        }
        let response = self
            .http
            .post(TOKEN_URL)
            .basic_auth(
                &self.credentials.client_id,
                Some(&self.credentials.client_secret),
            )
            .header(header::ACCEPT, "application/json; charset=utf-8")
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(ImportError::Upstream(format!(
                "token request failed: HTTP {}",
                response.status()
            )));
        }
        let body: TokenResponse = response.json().await?;
        let lifetime = Duration::from_secs(body.expires_in.unwrap_or(3_600));
        let valid_until = Instant::now()
            + lifetime
                .saturating_sub(TOKEN_REFRESH_MARGIN)
                .max(Duration::from_secs(30));
        *cached = Some((body.access_token.clone(), valid_until));
        Ok(body.access_token)
    }

    async fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, ImportError> {
        let parsed =
            ::url::Url::parse(url).map_err(|_| ImportError::Upstream("bad API url".into()))?;
        if parsed.scheme() != "https" || parsed.host_str() != Some(API_HOST) {
            return Err(ImportError::Upstream("API pointed outside its host".into()));
        }
        let mut refreshed = false;
        loop {
            let token = self.token(refreshed).await?;
            let response = self
                .http
                .get(parsed.clone())
                .header(header::AUTHORIZATION, format!("OAuth {token}"))
                .header(header::ACCEPT, "application/json; charset=utf-8")
                .send()
                .await?;
            match response.status() {
                StatusCode::UNAUTHORIZED if !refreshed => refreshed = true,
                s if s.is_success() => return Ok(response.json().await?),
                s => return Err(status_error(s)),
            }
        }
    }

    pub async fn resolve(&self, link: &Link, max_tracks: usize) -> Result<Resolved, ImportError> {
        let resource: Value = match link {
            Link::Page { url, .. } | Link::Short(url) => {
                let mut resolve =
                    ::url::Url::parse(&format!("{API_BASE}/resolve")).expect("static url");
                resolve.query_pairs_mut().append_pair("url", url.as_str());
                self.get(resolve.as_str()).await?
            }
            Link::Api { .. } => self.get(&link.target()).await?,
        };
        let link_secret = match link {
            Link::Api { secret_token, .. } => secret_token.clone(),
            _ => None,
        };
        match resource.get("kind").and_then(Value::as_str) {
            Some("track") => {
                let track: Track = parse(resource)?;
                let (tracks, unavailable) = split(vec![track], link_secret.as_deref());
                Ok(Resolved::Tracks {
                    collection: None,
                    tracks,
                    unavailable,
                    missing: 0,
                })
            }
            Some("playlist") => {
                let playlist: Playlist = parse(resource)?;
                self.playlist(playlist, link_secret, max_tracks).await
            }
            _ => Err(ImportError::Unsupported),
        }
    }

    async fn playlist(
        &self,
        playlist: Playlist,
        link_secret: Option<String>,
        max_tracks: usize,
    ) -> Result<Resolved, ImportError> {
        let expected = playlist.track_count.unwrap_or(0);
        if expected > max_tracks {
            return Err(ImportError::TooLarge);
        }
        let secret = playlist.secret_token.clone().or(link_secret);
        let mut first = ::url::Url::parse(&format!("{API_BASE}/playlists/{}/tracks", playlist.id))
            .expect("static url");
        {
            let mut query = first.query_pairs_mut();
            query
                .append_pair("linked_partitioning", "true")
                .append_pair("limit", &PAGE_SIZE.to_string())
                .append_pair("access", "playable,preview,blocked");
            if let Some(secret) = &secret {
                query.append_pair("secret_token", secret);
            }
        }

        let mut tracks = Vec::new();
        let mut next = Some(first.to_string());
        while let Some(url) = next {
            let page: Page = self.get(&url).await?;
            let received = page.collection.len();
            // Entries that fail to parse are deleted/private stubs; they count as missing.
            tracks.extend(
                page.collection
                    .into_iter()
                    .filter_map(|v| serde_json::from_value::<Track>(v).ok()),
            );
            if tracks.len() > max_tracks {
                return Err(ImportError::TooLarge);
            }
            next = page.next_href.filter(|_| received > 0);
        }

        let missing = expected.saturating_sub(tracks.len());
        let (tracks, unavailable) = split(tracks, None);
        Ok(Resolved::Tracks {
            collection: Some(Collection {
                title: playlist.title.clone().unwrap_or_default(),
                kind: collection_kind(&playlist),
            }),
            tracks,
            unavailable,
            missing,
        })
    }
}

fn parse<T: DeserializeOwned>(value: Value) -> Result<T, ImportError> {
    serde_json::from_value(value).map_err(|e| ImportError::Upstream(e.to_string()))
}

fn split(tracks: Vec<Track>, secret_token: Option<&str>) -> (Vec<TrackInfo>, Vec<Unavailable>) {
    let mut playable = Vec::new();
    let mut unavailable = Vec::new();
    for track in tracks {
        match track.unavailable_reason() {
            Some(reason) => unavailable.push(Unavailable {
                title: track.title,
                reason,
            }),
            None => playable.push(track.into_info(secret_token)),
        }
    }
    (playable, unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(json: serde_json::Value) -> Track {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn maps_unavailable_reasons() {
        let base = serde_json::json!({"id": 1, "embeddable_by": "all", "access": "playable", "streamable": true});
        assert_eq!(track(base.clone()).unavailable_reason(), None);
        let with = |k: &str, v: serde_json::Value| {
            let mut t = base.clone();
            t[k] = v;
            track(t).unavailable_reason()
        };
        assert_eq!(
            with("embeddable_by", "me".into()),
            Some(UnavailableReason::NotEmbeddable)
        );
        assert_eq!(
            with("access", "blocked".into()),
            Some(UnavailableReason::Blocked)
        );
        assert_eq!(
            with("access", "preview".into()),
            Some(UnavailableReason::PreviewOnly)
        );
        assert_eq!(
            with("streamable", false.into()),
            Some(UnavailableReason::NotStreamable)
        );
    }

    #[test]
    fn builds_track_info() {
        let info = track(serde_json::json!({
            "id": 293,
            "title": "Flickermood",
            "duration": 213000,
            "permalink_url": "https://soundcloud.com/forss/flickermood",
            "artwork_url": null,
            "user": {"username": "Forss", "avatar_url": "https://i1.sndcdn.com/avatars-1-large.jpg"},
            "secret_token": "s-abc"
        }))
        .into_info(None);
        assert_eq!(
            info.widget_url,
            "https://api.soundcloud.com/tracks/293?secret_token=s-abc"
        );
        assert_eq!(
            info.artwork.as_deref(),
            Some("https://i1.sndcdn.com/avatars-1-t500x500.jpg")
        );
        assert_eq!(info.artist, "Forss");
        assert_eq!(info.duration_ms, Some(213000));
    }

    #[test]
    fn detects_albums() {
        let playlist = |pt: Option<&str>, st: Option<&str>| Playlist {
            id: 1,
            title: None,
            playlist_type: pt.map(Into::into),
            set_type: st.map(Into::into),
            track_count: None,
            secret_token: None,
        };
        assert_eq!(
            collection_kind(&playlist(Some("album"), None)),
            CollectionKind::Album
        );
        assert_eq!(
            collection_kind(&playlist(None, Some("ep"))),
            CollectionKind::Album
        );
        assert_eq!(
            collection_kind(&playlist(Some("playlist"), Some(""))),
            CollectionKind::Playlist
        );
        assert_eq!(
            collection_kind(&playlist(None, None)),
            CollectionKind::Playlist
        );
    }
}
