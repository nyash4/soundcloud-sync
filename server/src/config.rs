use std::env;
use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, bail};

use crate::room::Timing;

#[derive(Debug, Clone)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub db_path: PathBuf,
    pub static_dir: PathBuf,
    pub soundcloud: Option<SoundCloudCredentials>,
    pub timing: Timing,
    pub max_import_tracks: usize,
    /// Take the client address from `X-Forwarded-For` (set when running behind a reverse proxy).
    pub trust_proxy: bool,
}

#[derive(Clone)]
pub struct SoundCloudCredentials {
    pub client_id: String,
    pub client_secret: String,
}

impl std::fmt::Debug for SoundCloudCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SoundCloudCredentials")
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

fn var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|v| !v.trim().is_empty())
}

fn parse<T: FromStr>(name: &str, default: T) -> anyhow::Result<T>
where
    T::Err: std::fmt::Display,
{
    match var(name) {
        None => Ok(default),
        Some(raw) => raw
            .trim()
            .parse()
            .map_err(|e| anyhow::anyhow!("{e}"))
            .with_context(|| format!("invalid {name}={raw:?}")),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let soundcloud = match (var("SC_CLIENT_ID"), var("SC_CLIENT_SECRET")) {
            (Some(client_id), Some(client_secret)) => Some(SoundCloudCredentials {
                client_id,
                client_secret,
            }),
            (None, None) => None,
            _ => bail!("set both SC_CLIENT_ID and SC_CLIENT_SECRET, or neither"),
        };
        let timing = Timing {
            ready_timeout_ms: parse("READY_TIMEOUT_MS", 20_000)?,
            end_timeout_ms: parse("END_TIMEOUT_MS", 30_000)?,
            heartbeat_timeout_ms: parse("HEARTBEAT_TIMEOUT_MS", 15_000)?,
            start_lead_ms: parse("START_LEAD_MS", 1_500)?,
        };
        if [
            timing.ready_timeout_ms,
            timing.end_timeout_ms,
            timing.heartbeat_timeout_ms,
            timing.start_lead_ms,
        ]
        .iter()
        .any(|&ms| ms < 0)
        {
            bail!("timeouts must not be negative");
        }
        let max_import_tracks = parse("MAX_IMPORT_TRACKS", 500)?;
        if max_import_tracks == 0 {
            bail!("MAX_IMPORT_TRACKS must be positive");
        }
        Ok(Self {
            host: parse("HOST", IpAddr::from([0, 0, 0, 0]))?,
            port: parse("PORT", 8080)?,
            db_path: var("DB_PATH").map_or_else(|| "./data/scsync.db".into(), PathBuf::from),
            static_dir: var("STATIC_DIR").map_or_else(|| "../web/dist".into(), PathBuf::from),
            soundcloud,
            timing,
            max_import_tracks,
            trust_proxy: parse("TRUST_PROXY", false)?,
        })
    }
}
