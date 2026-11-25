//! FileHash: parallel file hashing and manifest verification.

pub mod algo;
pub mod manifest;
pub mod pool;
pub mod walk;

/// Process exit statuses.
pub mod exit {
    pub const OK: u8 = 0;
    /// Mismatch, missing file or read error.
    pub const FAILURE: u8 = 1;
    pub const USAGE: u8 = 2;
}
