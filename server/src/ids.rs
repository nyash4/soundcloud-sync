use rand::RngExt;
use rand::distr::Alphanumeric;
use sha2::{Digest, Sha256};

/// Random base62 string from the thread-local CSPRNG.
pub fn base62(len: usize) -> String {
    rand::rng()
        .sample_iter(Alphanumeric)
        .take(len)
        .map(char::from)
        .collect()
}

pub fn room_id() -> String {
    base62(10)
}

pub fn room_token() -> String {
    base62(24)
}

pub fn entry_id() -> String {
    base62(8)
}

pub fn participant_id() -> String {
    base62(6)
}

pub fn is_room_id(s: &str) -> bool {
    s.len() == 10 && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub fn token_hash(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Compares token hashes without short-circuiting, so timing leaks nothing about the stored hash.
pub fn token_matches(token: &str, expected: &[u8; 32]) -> bool {
    let actual = token_hash(token);
    actual
        .iter()
        .zip(expected)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}
