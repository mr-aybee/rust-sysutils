//! Collect files from command-line inputs in a deterministic order.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// A file to hash and the path string written to the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileItem {
    pub path: PathBuf,
    pub display: String,
}

#[derive(Debug, Default)]
pub struct Walk {
    pub files: Vec<FileItem>,
    /// Paths that could not be read (permission denied, broken entries).
    pub errors: Vec<(String, String)>,
    /// Symbolic links encountered and not followed.
    pub skipped_symlinks: usize,
}

/// Manifest path: `/` separators on every platform.
pub fn display_path(p: &Path) -> Result<String, String> {
    let s = p
        .to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {}", p.display()))?;
    Ok(if cfg!(windows) {
        s.replace('\\', "/")
    } else {
        s.to_string()
    })
}

/// Expand inputs: files are taken as given; directories are walked recursively.
/// Output is sorted by manifest path so manifests are reproducible.
pub fn collect(inputs: &[PathBuf], follow_symlinks: bool, hidden: bool) -> Walk {
    let mut walk = Walk::default();
    let mut seen_dirs = HashSet::new();
    for input in inputs {
        let meta = if follow_symlinks {
            fs::metadata(input)
        } else {
            fs::symlink_metadata(input)
        };
        match meta {
            Err(e) => walk
                .errors
                .push((input.display().to_string(), e.to_string())),
            // An explicitly named symlink to a file is hashed (like sha256sum does).
            Ok(m) if m.file_type().is_symlink() => match fs::metadata(input) {
                Ok(t) if t.is_file() => push_file(&mut walk, input),
                Ok(_) => walk.skipped_symlinks += 1,
                Err(e) => walk
                    .errors
                    .push((input.display().to_string(), e.to_string())),
            },
            Ok(m) if m.is_dir() => {
                walk_dir(input, follow_symlinks, hidden, &mut seen_dirs, &mut walk)
            }
            Ok(m) if m.is_file() => push_file(&mut walk, input),
            Ok(_) => walk.errors.push((
                input.display().to_string(),
                "not a regular file or directory".into(),
            )),
        }
    }
    walk.files.sort_by(|a, b| a.display.cmp(&b.display));
    walk.files.dedup_by(|a, b| a.display == b.display);
    walk
}

fn push_file(walk: &mut Walk, p: &Path) {
    match display_path(p) {
        Ok(display) => walk.files.push(FileItem {
            path: p.to_path_buf(),
            display,
        }),
        Err(e) => walk.errors.push((p.display().to_string(), e)),
    }
}

fn walk_dir(dir: &Path, follow: bool, hidden: bool, seen: &mut HashSet<PathBuf>, walk: &mut Walk) {
    // Guard against symlink cycles when following links.
    if follow {
        match fs::canonicalize(dir) {
            Ok(c) => {
                if !seen.insert(c) {
                    return;
                }
            }
            Err(e) => {
                walk.errors.push((dir.display().to_string(), e.to_string()));
                return;
            }
        }
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            walk.errors.push((dir.display().to_string(), e.to_string()));
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                walk.errors.push((dir.display().to_string(), e.to_string()));
                continue;
            }
        };
        let path = entry.path();
        if !hidden && entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(e) => {
                walk.errors
                    .push((path.display().to_string(), e.to_string()));
                continue;
            }
        };
        if ft.is_symlink() {
            if !follow {
                walk.skipped_symlinks += 1;
                continue;
            }
            match fs::metadata(&path) {
                Ok(m) if m.is_dir() => walk_dir(&path, follow, hidden, seen, walk),
                Ok(m) if m.is_file() => push_file(walk, &path),
                Ok(_) => {}
                Err(e) => walk
                    .errors
                    .push((path.display().to_string(), e.to_string())),
            }
        } else if ft.is_dir() {
            walk_dir(&path, follow, hidden, seen, walk);
        } else if ft.is_file() {
            push_file(walk, &path);
        }
        // Sockets, FIFOs and devices are skipped silently: hashing them is not meaningful.
    }
}
