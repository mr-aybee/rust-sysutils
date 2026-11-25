//! FileHash: parallel file hashing and manifest verification.

pub mod algo;
pub mod manifest;
pub mod pool;
pub mod walk;

/// Process exit statuses.
pub mod exit {
    /// Everything succeeded.
    pub const OK: u8 = 0;
    /// Mismatch, missing file or read error.
    pub const FAILURE: u8 = 1;
    /// Invalid usage or an unreadable/malformed manifest.
    pub const USAGE: u8 = 2;
}
