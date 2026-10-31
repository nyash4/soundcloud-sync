//! Credential-less mode: metadata from SoundCloud's public oEmbed endpoint.

use futures_util::{StreamExt, stream};
use serde::Deserialize;

use super::url::{self, Kind, Link};
use super::{ImportError, Resolved, status_error};
use crate::protocol::{Collection, CollectionKind, TrackInfo, Unavailable, UnavailableReason};

const ENDPOINT: &str = "https://soundcloud.com/oembed";
const CONCURRENCY: usize = 6;

#[derive(Deserialize)]
struct OEmbed {
    title: String,
    author_name: String,
    html: String,
    thumbnail_url: Option<String>,
}

struct Info {
    title: String,
    artist: String,
    artwork: Option<String>,
    resource: Option<Link>,
}

async fn fetch(http: &reqwest::Client, target: &str) -> Result<Info, ImportError> {
    let response = http
        .get(ENDPOINT)
        .query(&[("format", "json"), ("url", target)])
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(status_error(response.status()));
    }
    let oembed: OEmbed = response.json().await?;
    // oEmbed titles read "Track by Artist"; the artist is reported separately.
    let suffix = format!(" by {}", oembed.author_name);
    let title = oembed
        .title
        .strip_suffix(&suffix)
        .unwrap_or(&oembed.title)
        .to_owned();
    Ok(Info {
        title,
        artist: oembed.author_name,
        artwork: oembed.thumbnail_url.filter(|u| u.starts_with("https://")),
        resource: embedded_resource(&oembed.html),
    })
}

/// Extracts the `api.soundcloud.com` resource from the embed iframe's `src`.
fn embedded_resource(html: &str) -> Option<Link> {
    let start = html.find("src=\"")? + 5;
    let end = start + html[start..].find('"')?;
    let src = html[start..end].replace("&amp;", "&");
    let player = ::url::Url::parse(&src).ok()?;
    let inner = player
        .query_pairs()
        .find(|(k, _)| k == "url")
        .map(|(_, v)| v.into_owned())?;
    match url::parse(&inner) {
        Ok(link @ Link::Api { .. }) => Some(link),
        _ => None,
    }
}

fn track(info: Info, id: u64, secret_token: Option<&str>, permalink: Option<String>) -> TrackInfo {
    let widget_url = url::api_url(Kind::Track, id, secret_token);
    TrackInfo {
        track_id: id,
        permalink: permalink.unwrap_or_else(|| url::player_link(&widget_url)),
        widget_url,
        title: info.title,
        artist: info.artist,
        duration_ms: None,
        artwork: info.artwork,
    }
}

pub async fn resolve(
    http: &reqwest::Client,
    link: &Link,
    track_ids: Option<&[u64]>,
    max_tracks: usize,
) -> Result<Resolved, ImportError> {
    let mut info = fetch(http, &link.target()).await?;
    let permalink = match link {
        Link::Page { url, .. } => Some(url.to_string()),
        _ => None,
    };
    match info.resource.take() {
        Some(Link::Api {
            kind: Kind::Track,
            id,
            secret_token,
        }) => Ok(Resolved::Tracks {
            collection: None,
            tracks: vec![track(info, id, secret_token.as_deref(), permalink)],
            unavailable: Vec::new(),
            missing: 0,
        }),
        Some(Link::Api {
            kind: Kind::Playlist,
            id,
            secret_token,
        }) => {
            // oEmbed does not say whether a set is an album, so every collection is a playlist here.
            let collection = Collection {
                title: info.title,
                kind: CollectionKind::Playlist,
            };
            match track_ids {
                None => Ok(Resolved::NeedsExpansion {
                    collection,
                    playlist_url: url::api_url(Kind::Playlist, id, secret_token.as_deref()),
                }),
                Some(ids) if ids.len() > max_tracks => Err(ImportError::TooLarge),
                Some(ids) => Ok(expand(http, ids, collection).await),
            }
        }
        _ => Err(ImportError::Unsupported),
    }
}

/// Looks up each client-supplied track id via oEmbed, so only real SoundCloud tracks get queued.
async fn expand(http: &reqwest::Client, ids: &[u64], collection: Collection) -> Resolved {
    let results: Vec<_> = stream::iter(ids.iter().copied())
        .map(|id| async move {
            let target = url::api_url(Kind::Track, id, None);
            (id, fetch(http, &target).await)
        })
        .buffered(CONCURRENCY)
        .collect()
        .await;
    let mut tracks = Vec::new();
    let mut unavailable = Vec::new();
    for (id, result) in results {
        match result {
            Ok(info) => tracks.push(track(info, id, None, None)),
            Err(_) => unavailable.push(Unavailable {
                title: None,
                reason: UnavailableReason::Unknown,
            }),
        }
    }
    Resolved::Tracks {
        collection: Some(collection),
        tracks,
        unavailable,
        missing: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_resource_in_embed_html() {
        let html = r#"<iframe width="100%" height="400" src="https://w.soundcloud.com/player/?visual=true&url=https%3A%2F%2Fapi.soundcloud.com%2Ftracks%2F293&show_artwork=true"></iframe>"#;
        assert_eq!(
            embedded_resource(html),
            Some(Link::Api {
                kind: Kind::Track,
                id: 293,
                secret_token: None
            })
        );
        let secret = r#"<iframe src="https://w.soundcloud.com/player/?visual=true&amp;url=https%3A%2F%2Fapi.soundcloud.com%2Fplaylists%2F18%3Fsecret_token%3Ds-abc"></iframe>"#;
        assert_eq!(
            embedded_resource(secret),
            Some(Link::Api {
                kind: Kind::Playlist,
                id: 18,
                secret_token: Some("s-abc".into())
            })
        );
        let user = r#"<iframe src="https://w.soundcloud.com/player/?url=https%3A%2F%2Fapi.soundcloud.com%2Fusers%2F183"></iframe>"#;
        assert_eq!(embedded_resource(user), None);
    }
}
