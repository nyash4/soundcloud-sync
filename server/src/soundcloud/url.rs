//! Link validation. Only allow-listed SoundCloud hosts pass; everything is canonicalized
//! (https, no tracking query, no fragment) before the server ever makes a request with it.

use url::Url;

use super::ImportError;

const MAX_URL_LEN: usize = 2_048;

/// First path segments of soundcloud.com that are site sections rather than users.
const RESERVED: &[&str] = &[
    "charts",
    "discover",
    "feed",
    "jobs",
    "logout",
    "messages",
    "mobile",
    "notifications",
    "pages",
    "people",
    "pro",
    "search",
    "settings",
    "signin",
    "stations",
    "stream",
    "tags",
    "terms-of-use",
    "upload",
    "you",
];

/// Second path segments that are profile tabs, not track slugs.
const PROFILE_TABS: &[&str] = &[
    "albums",
    "comments",
    "followers",
    "following",
    "likes",
    "playlists",
    "popular-tracks",
    "reposts",
    "sets",
    "spotlight",
    "toptracks",
    "tracks",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Track,
    Playlist,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// A soundcloud.com page, canonicalized to `https://soundcloud.com/...`.
    Page { url: Url, kind: Kind },
    /// An `api.soundcloud.com` resource.
    Api {
        kind: Kind,
        id: u64,
        secret_token: Option<String>,
    },
    /// An `on.soundcloud.com` short link that still has to be expanded.
    Short(Url),
}

impl Link {
    /// The URL to hand to SoundCloud's resolve/oEmbed endpoints.
    pub fn target(&self) -> String {
        match self {
            Link::Page { url, .. } | Link::Short(url) => url.to_string(),
            Link::Api {
                kind,
                id,
                secret_token,
            } => api_url(*kind, *id, secret_token.as_deref()),
        }
    }
}

pub fn api_url(kind: Kind, id: u64, secret_token: Option<&str>) -> String {
    let path = match kind {
        Kind::Track => "tracks",
        Kind::Playlist => "playlists",
    };
    match secret_token {
        Some(token) => format!("https://api.soundcloud.com/{path}/{id}?secret_token={token}"),
        None => format!("https://api.soundcloud.com/{path}/{id}"),
    }
}

/// Public link that opens a track for which only the API id is known.
pub fn player_link(widget_url: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(widget_url.as_bytes()).collect();
    format!("https://w.soundcloud.com/player/?url={encoded}")
}

fn is_secret_token(s: &str) -> bool {
    (3..=64).contains(&s.len())
        && s.starts_with("s-")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn parse(input: &str) -> Result<Link, ImportError> {
    let input = input.trim();
    if input.is_empty() || input.len() > MAX_URL_LEN {
        return Err(ImportError::BadUrl);
    }
    let with_scheme = if input.contains("://") {
        input.to_owned()
    } else {
        format!("https://{input}")
    };
    let url = Url::parse(&with_scheme).map_err(|_| ImportError::BadUrl)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(ImportError::BadUrl);
    }
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).collect())
        .unwrap_or_default();

    match url.host_str().unwrap_or_default() {
        "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" => page(&segments),
        "on.soundcloud.com" => match segments.as_slice() {
            [code] if code.bytes().all(|b| b.is_ascii_alphanumeric()) => {
                let short = Url::parse(&format!("https://on.soundcloud.com/{code}"))
                    .map_err(|_| ImportError::BadUrl)?;
                Ok(Link::Short(short))
            }
            _ => Err(ImportError::BadUrl),
        },
        "api.soundcloud.com" => api(&url, &segments),
        _ => Err(ImportError::BadUrl),
    }
}

fn page(segments: &[&str]) -> Result<Link, ImportError> {
    let kind = match segments {
        [first, ..] if RESERVED.contains(first) => return Err(ImportError::Unsupported),
        [_, tab] if PROFILE_TABS.contains(tab) => return Err(ImportError::Unsupported),
        [_, _] => Kind::Track,
        [_, _, secret] if is_secret_token(secret) => Kind::Track,
        [_, "sets", _] => Kind::Playlist,
        [_, "sets", _, secret] if is_secret_token(secret) => Kind::Playlist,
        _ => return Err(ImportError::Unsupported),
    };
    let url = Url::parse(&format!("https://soundcloud.com/{}", segments.join("/")))
        .map_err(|_| ImportError::BadUrl)?;
    Ok(Link::Page { url, kind })
}

fn api(url: &Url, segments: &[&str]) -> Result<Link, ImportError> {
    let (kind, id) = match segments {
        ["tracks", id] => (Kind::Track, id),
        ["playlists", id] => (Kind::Playlist, id),
        ["users", ..] => return Err(ImportError::Unsupported),
        _ => return Err(ImportError::BadUrl),
    };
    let id = id.parse().map_err(|_| ImportError::BadUrl)?;
    let secret_token = url
        .query_pairs()
        .find(|(k, _)| k == "secret_token")
        .map(|(_, v)| v.into_owned())
        .filter(|v| is_secret_token(v));
    Ok(Link::Api {
        kind,
        id,
        secret_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page_url(input: &str) -> (String, Kind) {
        match parse(input) {
            Ok(Link::Page { url, kind }) => (url.to_string(), kind),
            other => panic!("{input}: expected page link, got {other:?}"),
        }
    }

    #[test]
    fn canonicalizes_track_pages() {
        for input in [
            "https://soundcloud.com/forss/flickermood",
            "https://www.soundcloud.com/forss/flickermood/",
            "https://m.soundcloud.com/forss/flickermood?si=abc&utm_source=clipboard&utm_medium=text",
            "soundcloud.com/forss/flickermood#t=1:00",
            "  https://SoundCloud.com/forss/flickermood?ref=clipboard&p=i&c=1  ",
        ] {
            assert_eq!(
                page_url(input),
                (
                    "https://soundcloud.com/forss/flickermood".into(),
                    Kind::Track
                ),
                "{input}"
            );
        }
    }

    #[test]
    fn recognizes_sets_and_secret_links() {
        assert_eq!(
            page_url("https://soundcloud.com/forss/sets/soulhack?si=x"),
            (
                "https://soundcloud.com/forss/sets/soulhack".into(),
                Kind::Playlist
            )
        );
        assert_eq!(
            page_url("https://soundcloud.com/user/track/s-AbC123"),
            (
                "https://soundcloud.com/user/track/s-AbC123".into(),
                Kind::Track
            )
        );
        assert_eq!(
            page_url("https://soundcloud.com/user/sets/mix/s-AbC123"),
            (
                "https://soundcloud.com/user/sets/mix/s-AbC123".into(),
                Kind::Playlist
            )
        );
    }

    #[test]
    fn accepts_api_resources() {
        assert_eq!(
            parse("https://api.soundcloud.com/tracks/293").unwrap(),
            Link::Api {
                kind: Kind::Track,
                id: 293,
                secret_token: None
            }
        );
        let link =
            parse("https://api.soundcloud.com/playlists/18?secret_token=s-xyz&si=1").unwrap();
        assert_eq!(
            link,
            Link::Api {
                kind: Kind::Playlist,
                id: 18,
                secret_token: Some("s-xyz".into())
            }
        );
        assert_eq!(
            link.target(),
            "https://api.soundcloud.com/playlists/18?secret_token=s-xyz"
        );
    }

    #[test]
    fn short_links_need_expansion() {
        assert_eq!(
            parse("https://on.soundcloud.com/AbC123?si=zzz").unwrap(),
            Link::Short(Url::parse("https://on.soundcloud.com/AbC123").unwrap())
        );
    }

    #[test]
    fn rejects_foreign_or_unsafe_urls() {
        for input in [
            "",
            "https://evil.com/forss/flickermood",
            "https://soundcloud.com.evil.com/forss/flickermood",
            "https://evilsoundcloud.com/forss/flickermood",
            "http://soundcloud.com/forss/flickermood",
            "ftp://soundcloud.com/forss/flickermood",
            "javascript:alert(1)",
            "https://user:pw@soundcloud.com/forss/flickermood",
            "https://soundcloud.com:8443/forss/flickermood",
            "https://api.soundcloud.com/tracks/abc",
            "https://api.soundcloud.com/resolve?url=https://evil.com",
            "https://on.soundcloud.com/",
            "https://on.soundcloud.com/a/b",
            "https://127.0.0.1/forss/flickermood",
            "https://w.soundcloud.com/player/?url=https://api.soundcloud.com/tracks/1",
        ] {
            assert!(
                matches!(parse(input), Err(ImportError::BadUrl)),
                "{input} should be bad_url"
            );
        }
    }

    #[test]
    fn rejects_profiles_and_site_sections() {
        for input in [
            "https://soundcloud.com/",
            "https://soundcloud.com/forss",
            "https://soundcloud.com/forss/likes",
            "https://soundcloud.com/forss/sets",
            "https://soundcloud.com/discover/sets/weekly",
            "https://soundcloud.com/search/sounds?q=x",
            "https://soundcloud.com/forss/flickermood/comments",
            "https://api.soundcloud.com/users/183",
        ] {
            assert!(
                matches!(parse(input), Err(ImportError::Unsupported)),
                "{input} should be unsupported"
            );
        }
    }
}
