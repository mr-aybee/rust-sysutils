//! Parallel file hashing with bounded memory.
//!
//! `workers` threads pull the next file index from a shared atomic counter. Each
//! worker owns a single reusable read buffer, so peak memory is
//! `workers × buffer_size` regardless of file sizes or counts. Results are
//! written into their input slot, which keeps output order deterministic.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::algo::Algorithm;

/// Read buffer size per worker (1 MiB).
pub const DEFAULT_BUFFER: usize = 1 << 20;

/// Files at least this large may be hashed with BLAKE3's internal multithreading.
pub const LARGE_FILE: u64 = 16 << 20;

/// Hash a large file using BLAKE3's memory-mapped, multithreaded mode.
/// Used only when workers would otherwise sit idle (fewer files than workers).
fn hash_blake3_parallel(path: &Path) -> io::Result<(String, u64)> {
    let mut h = blake3::Hasher::new();
    h.update_mmap_rayon(path)?;
    let len = h.count();
    Ok((h.finalize().to_hex().to_string(), len))
}

/// Hash one file by streaming it through `buf`.
pub fn hash_file(path: &Path, algo: Algorithm, buf: &mut [u8]) -> io::Result<(String, u64)> {
    let mut file = File::open(path)?;
    let mut hasher = algo.hasher();
    let mut total = 0u64;

    loop {
        let n = match file.read(buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };

        hasher.update(&buf[..n]);
        total += n as u64;
    }

    Ok((hasher.finalize_hex(), total))
}

/// Outcome per input path, in input order.
pub type HashResult = Result<(String, u64), String>;

/// Hash `paths` with `workers` threads. `on_done(index, result)` is called from
/// worker threads as each file completes (for progress); it must be cheap.
///
/// `allow_mmap`: when there are fewer files than workers, BLAKE3 may memory-map
/// large files and hash them with internal multithreading. Mapped pages count
/// toward RSS (reclaimable page cache); pass `false` to always stream.
pub fn hash_all<P: AsRef<Path> + Sync>(
    paths: &[P],
    algo: Algorithm,
    workers: usize,
    buffer_size: usize,
    allow_mmap: bool,
    on_done: &(dyn Fn(usize, &HashResult) + Sync),
) -> Vec<HashResult> {
    // With fewer files than requested workers, let BLAKE3 parallelize within large files.
    let intra_file = allow_mmap && algo == Algorithm::Blake3 && workers > paths.len();
    let workers = workers.clamp(1, paths.len().max(1));
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<HashResult>>> =
        (0..paths.len()).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                let mut buf = vec![0u8; buffer_size.max(4096)];

                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = paths.get(i) else { break };
                    let path = path.as_ref();
                    let large = intra_file
                        && std::fs::metadata(path)
                            .is_ok_and(|m| m.is_file() && m.len() >= LARGE_FILE);

                    let r = if large {
                        hash_blake3_parallel(path)
                    } else {
                        hash_file(path, algo, &mut buf)
                    };

                    let r = r.map_err(|e| e.to_string());
                    on_done(i, &r);

                    if let Ok(mut slot) = slots[i].lock() {
                        *slot = Some(r);
                    }
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|m| {
            m.into_inner()
                .ok()
                .flatten()
                .unwrap_or_else(|| Err("internal error: result missing".into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn parallel_matches_sequential_and_keeps_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();

        for i in 0..50 {
            let p = dir.path().join(format!("f{i}"));
            let mut f = File::create(&p).unwrap();
            f.write_all(&vec![i as u8; i * 1000 + 1]).unwrap();
            paths.push(p);
        }

        paths.push(dir.path().join("missing"));
        let seq = hash_all(&paths, Algorithm::Blake3, 1, 4096, true, &|_, _| {});
        let par = hash_all(&paths, Algorithm::Blake3, 8, 4096, true, &|_, _| {});
        assert_eq!(seq, par);
        assert!(par[50].is_err());
        assert_eq!(par[3].as_ref().unwrap().1, 3001);
        assert_eq!(
            par[0].as_ref().unwrap().0,
            crate::algo::digest(Algorithm::Blake3, &[0u8])
        );
    }

    #[test]
    fn intra_file_parallel_matches_streaming() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("large");
        let data: Vec<u8> = (0..(LARGE_FILE as usize + 12_345))
            .map(|i| (i % 251) as u8)
            .collect();
        std::fs::write(&p, &data).unwrap();
        let parallel = hash_all(&[&p], Algorithm::Blake3, 8, 4096, true, &|_, _| {});
        let streamed = hash_all(&[&p], Algorithm::Blake3, 1, 4096, true, &|_, _| {});
        assert_eq!(parallel, streamed);
        assert_eq!(
            parallel[0].as_ref().unwrap().0,
            crate::algo::digest(Algorithm::Blake3, &data)
        );
    }

    #[test]
    fn small_buffer_same_digest() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x");
        std::fs::write(&p, vec![7u8; 100_000]).unwrap();
        let mut small = vec![0u8; 7];
        let mut big = vec![0u8; 1 << 20];
        assert_eq!(
            hash_file(&p, Algorithm::Sha256, &mut small).unwrap(),
            hash_file(&p, Algorithm::Sha256, &mut big).unwrap()
        );
    }
}
