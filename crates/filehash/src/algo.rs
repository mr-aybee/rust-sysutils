//! Supported hash algorithms behind a small enum (no dynamic dispatch in the hot loop).

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    Sha256,
    Sha512,
    Blake3,
}

impl Algorithm {
    pub const ALL: [Algorithm; 3] = [Algorithm::Sha256, Algorithm::Sha512, Algorithm::Blake3];

    /// Length of the hex digest.
    pub fn hex_len(self) -> usize {
        match self {
            Algorithm::Sha256 | Algorithm::Blake3 => 64,
            Algorithm::Sha512 => 128,
        }
    }

    /// Name used in BSD-style tagged lines (`SHA256 (file) = ...`).
    pub fn tag(self) -> &'static str {
        match self {
            Algorithm::Sha256 => "SHA256",
            Algorithm::Sha512 => "SHA512",
            Algorithm::Blake3 => "BLAKE3",
        }
    }

    pub fn from_tag(t: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|a| a.tag().eq_ignore_ascii_case(t))
    }

    pub fn hasher(self) -> Hasher {
        match self {
            Algorithm::Sha256 => Hasher::Ring(ring::digest::Context::new(&ring::digest::SHA256)),
            Algorithm::Sha512 => Hasher::Ring(ring::digest::Context::new(&ring::digest::SHA512)),
            Algorithm::Blake3 => Hasher::Blake3(blake3::Hasher::new()),
        }
    }
}

impl fmt::Display for Algorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Algorithm::Sha256 => "sha256",
            Algorithm::Sha512 => "sha512",
            Algorithm::Blake3 => "blake3",
        })
    }
}

impl FromStr for Algorithm {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().replace('-', "").as_str() {
            "sha256" => Ok(Algorithm::Sha256),
            "sha512" => Ok(Algorithm::Sha512),
            "blake3" | "b3" => Ok(Algorithm::Blake3),
            _ => Err(format!(
                "unknown algorithm '{s}' (expected sha256, sha512, blake3)"
            )),
        }
    }
}

/// SHA-2 uses ring's assembly-optimized implementation (AVX/SSSE3/SHA-NI/ARMv8).
// One hasher lives on a worker's stack per file and is never stored in collections,
// so variant size differences do not matter; boxing would add an allocation per file.
#[allow(clippy::large_enum_variant)]
pub enum Hasher {
    Ring(ring::digest::Context),
    Blake3(blake3::Hasher),
}

impl Hasher {
    #[inline]
    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Ring(h) => h.update(data),
            Hasher::Blake3(h) => {
                h.update(data);
            }
        }
    }

    pub fn finalize_hex(self) -> String {
        match self {
            Hasher::Ring(h) => hex(h.finish().as_ref()),
            Hasher::Blake3(h) => h.finalize().to_hex().to_string(),
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);

    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0xf)] as char);
    }

    s
}

/// Hash a byte slice (used by tests and benchmarks).
pub fn digest(algo: Algorithm, data: &[u8]) -> String {
    let mut h = algo.hasher();
    h.update(data);
    h.finalize_hex()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_vectors() {
        // Empty input.
        assert_eq!(
            digest(Algorithm::Sha256, b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            digest(Algorithm::Blake3, b""),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        assert!(
            digest(Algorithm::Sha512, b"")
                .starts_with("cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce")
        );

        // "abc" (FIPS 180-2 example).
        assert_eq!(
            digest(Algorithm::Sha256, b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn parse_names() {
        assert_eq!("SHA-256".parse::<Algorithm>(), Ok(Algorithm::Sha256));
        assert_eq!("b3".parse::<Algorithm>(), Ok(Algorithm::Blake3));
        assert!("md5".parse::<Algorithm>().is_err());
        assert_eq!(Algorithm::from_tag("blake3"), Some(Algorithm::Blake3));
    }

    #[test]
    fn hex_lengths() {
        for a in Algorithm::ALL {
            assert_eq!(digest(a, b"x").len(), a.hex_len());
        }
    }
}
