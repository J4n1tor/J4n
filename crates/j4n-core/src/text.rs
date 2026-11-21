//! Text cut for a line and hashes as hex: one rule each, used by every crate.

use sha2::{Digest, Sha256};

/// `s` cut to `n` characters, with `…` after the cut.
pub fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

/// `s` as one line (its whitespace one space), cut to `max` characters with the `…` counted.
pub fn line(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

/// The SHA-256 of `bytes`, its first 8 bytes in lowercase hex.
pub fn short_hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes).iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of `bytes`, in lowercase hex.
pub fn hex_hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
