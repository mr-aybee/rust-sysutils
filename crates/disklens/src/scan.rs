//! Parallel directory scan.
//!
//! Each directory is one task on rayon's work-stealing pool. Only directories are
//! materialized as nodes; files are folded into their parent's totals, and the
//! largest files are tracked in bounded per-directory lists merged upward. Memory
//! therefore grows with the number of directories, not files.

use crate::glob::Pattern;
use rayon::prelude::*;
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of individual error messages retained (all are counted).
pub const MAX_ERRORS_KEPT: usize = 100;

#[derive(Debug, Clone)]
pub struct Options {
    pub one_file_system: bool,
    pub exclude: Vec<Pattern>,
    pub top_files: usize,
    /// Rank files and directories by logical (apparent) size instead of allocated.
    pub apparent: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct DirNode {
    pub name: String,
    /// Path relative to the scan root, `/`-separated ("" for the root).
    pub path: String,
    /// Apparent size in bytes: file and symlink lengths (directories count as 0, like GNU du).
    pub logical: u64,
    /// Bytes allocated on disk (equal to `logical` where the OS does not report blocks).
    pub allocated: u64,
    pub files: u64,
    pub dirs: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DirNode>,
}

impl DirNode {
    pub fn size(&self, apparent: bool) -> u64 {
        if apparent {
            self.logical
        } else {
            self.allocated
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub logical: u64,
    pub allocated: u64,
}

impl FileEntry {
    fn key(&self, apparent: bool) -> u64 {
        if apparent {
            self.logical
        } else {
            self.allocated
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ScanResult {
    pub root: DirNode,
    pub largest_files: Vec<FileEntry>,
    pub errors: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub error_samples: Vec<ScanError>,
    /// Files skipped because another link to the same inode was already counted.
    pub hardlinks_deduplicated: u64,
    pub symlinks_skipped: u64,
    pub excluded: u64,
    /// Whether allocated sizes come from the OS (false on Windows: allocated = logical).
    pub allocated_supported: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanError {
    pub path: String,
    pub error: String,
}

struct Ctx<'a> {
    opts: &'a Options,
    root_dev: Option<u64>,
    seen_inodes: Mutex<HashSet<(u64, u64)>>,
    errors: AtomicU64,
    error_samples: Mutex<Vec<ScanError>>,
    hardlinks: AtomicU64,
    symlinks: AtomicU64,
    excluded: AtomicU64,
}

impl Ctx<'_> {
    fn error(&self, path: &Path, e: &std::io::Error) {
        self.errors.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut v) = self.error_samples.lock()
            && v.len() < MAX_ERRORS_KEPT
        {
            v.push(ScanError {
                path: path.display().to_string(),
                error: e.to_string(),
            });
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::fs::Metadata;
    use std::os::unix::fs::MetadataExt;
    pub const ALLOCATED_SUPPORTED: bool = true;
    pub fn allocated(m: &Metadata) -> u64 {
        m.blocks() * 512
    }
    pub fn dev(m: &Metadata) -> Option<u64> {
        Some(m.dev())
    }
    /// (device, inode) for files with more than one hard link.
    pub fn hardlink_id(m: &Metadata) -> Option<(u64, u64)> {
        (m.nlink() > 1).then(|| (m.dev(), m.ino()))
    }
}

#[cfg(not(unix))]
mod platform {
    use std::fs::Metadata;
    pub const ALLOCATED_SUPPORTED: bool = false;
    pub fn allocated(m: &Metadata) -> u64 {
        m.len()
    }
    pub fn dev(_: &Metadata) -> Option<u64> {
        None
    }
    pub fn hardlink_id(_: &Metadata) -> Option<(u64, u64)> {
        None
    }
}

fn join_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

/// Merge `b` into `a`, keeping the `n` largest by the chosen metric.
fn merge_top(a: &mut Vec<FileEntry>, mut b: Vec<FileEntry>, n: usize, apparent: bool) {
    a.append(&mut b);
    if a.len() > n {
        a.select_nth_unstable_by(n, |x, y| y.key(apparent).cmp(&x.key(apparent)));
        a.truncate(n);
    }
}

fn excluded(ctx: &Ctx<'_>, name: &str, rel: &str) -> bool {
    ctx.opts.exclude.iter().any(|p| p.matches(name, rel))
}

fn scan_dir(
    path: &Path,
    name: String,
    rel: String,
    meta: &Metadata,
    ctx: &Ctx<'_>,
) -> (DirNode, Vec<FileEntry>) {
    let mut node = DirNode {
        name,
        path: rel,
        // A directory's own entry occupies disk blocks (counted, like du), but its
        // apparent size is 0, matching GNU du --apparent-size.
        logical: 0,
        allocated: platform::allocated(meta),
        ..Default::default()
    };
    let mut top: Vec<FileEntry> = Vec::new();
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            ctx.error(path, &e);
            return (node, top);
        }
    };
    let mut subdirs: Vec<(PathBuf, String, String, Metadata)> = Vec::new();
    let n = ctx.opts.top_files;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                ctx.error(path, &e);
                continue;
            }
        };
        let child_path = entry.path();
        let fname = entry.file_name().to_string_lossy().into_owned();
        let child_rel = join_rel(&node.path, &fname);
        if !ctx.opts.exclude.is_empty() && excluded(ctx, &fname, &child_rel) {
            ctx.excluded.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        // DirEntry::metadata does not follow symlinks.
        let m = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                ctx.error(&child_path, &e);
                continue;
            }
        };
        let ft = m.file_type();
        if ft.is_symlink() {
            // Never followed; the link itself is counted (like du), but not as a file.
            ctx.symlinks.fetch_add(1, Ordering::Relaxed);
            node.logical += m.len();
            node.allocated += platform::allocated(&m);
            continue;
        }
        if ft.is_dir() {
            if ctx.opts.one_file_system
                && ctx.root_dev.is_some()
                && platform::dev(&m) != ctx.root_dev
            {
                continue;
            }
            subdirs.push((child_path, fname, child_rel, m));
            continue;
        }
        // Regular files and special files (their metadata size is usually 0).
        if let Some(id) = platform::hardlink_id(&m) {
            let fresh = ctx
                .seen_inodes
                .lock()
                .map(|mut s| s.insert(id))
                .unwrap_or(true);
            if !fresh {
                ctx.hardlinks.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        }
        let (logical, allocated) = (m.len(), platform::allocated(&m));
        node.logical += logical;
        node.allocated += allocated;
        node.files += 1;
        if n > 0 {
            let key = if ctx.opts.apparent {
                logical
            } else {
                allocated
            };
            // Only build the path string for files that can enter the list.
            let qualifies = top.len() < n * 2 || top.iter().any(|f| f.key(ctx.opts.apparent) < key);
            if qualifies {
                top.push(FileEntry {
                    path: child_rel,
                    logical,
                    allocated,
                });
                if top.len() >= n * 2 {
                    merge_top(&mut top, Vec::new(), n, ctx.opts.apparent);
                }
            }
        }
    }

    let results: Vec<(DirNode, Vec<FileEntry>)> = subdirs
        .into_par_iter()
        .map(|(p, name, rel, m)| scan_dir(&p, name, rel, &m, ctx))
        .collect();
    for (child, child_top) in results {
        node.logical += child.logical;
        node.allocated += child.allocated;
        node.files += child.files;
        node.dirs += child.dirs + 1;
        node.children.push(child);
        merge_top(&mut top, child_top, n, ctx.opts.apparent);
    }
    merge_top(&mut top, Vec::new(), n, ctx.opts.apparent);
    (node, top)
}

/// Scan `root`. A file root is reported as a root with a single file.
pub fn scan(root: &Path, opts: &Options) -> Result<ScanResult, String> {
    let meta =
        fs::symlink_metadata(root).map_err(|e| format!("cannot access {}: {e}", root.display()))?;
    let meta = if meta.file_type().is_symlink() {
        // An explicitly named symlink to a directory is followed (like `du dir/`).
        fs::metadata(root).map_err(|e| format!("cannot access {}: {e}", root.display()))?
    } else {
        meta
    };
    let ctx = Ctx {
        opts,
        root_dev: platform::dev(&meta),
        seen_inodes: Mutex::new(HashSet::new()),
        errors: AtomicU64::new(0),
        error_samples: Mutex::new(Vec::new()),
        hardlinks: AtomicU64::new(0),
        symlinks: AtomicU64::new(0),
        excluded: AtomicU64::new(0),
    };
    let display = root.display().to_string();
    let (root_node, mut top) = if meta.is_dir() {
        scan_dir(root, display, String::new(), &meta, &ctx)
    } else {
        let f = FileEntry {
            path: display.clone(),
            logical: meta.len(),
            allocated: platform::allocated(&meta),
        };
        let node = DirNode {
            name: display,
            logical: f.logical,
            allocated: f.allocated,
            files: 1,
            ..Default::default()
        };
        (node, vec![f])
    };
    top.sort_by(|a, b| {
        b.key(opts.apparent)
            .cmp(&a.key(opts.apparent))
            .then_with(|| a.path.cmp(&b.path))
    });
    top.truncate(opts.top_files);
    let mut root_node = root_node;
    sort_tree(&mut root_node, opts.apparent);
    Ok(ScanResult {
        root: root_node,
        largest_files: top,
        errors: ctx.errors.load(Ordering::Relaxed),
        error_samples: ctx.error_samples.into_inner().unwrap_or_default(),
        hardlinks_deduplicated: ctx.hardlinks.load(Ordering::Relaxed),
        symlinks_skipped: ctx.symlinks.load(Ordering::Relaxed),
        excluded: ctx.excluded.load(Ordering::Relaxed),
        allocated_supported: platform::ALLOCATED_SUPPORTED,
    })
}

/// Largest first; ties broken by name for deterministic output.
fn sort_tree(node: &mut DirNode, apparent: bool) {
    node.children.sort_by(|a, b| {
        b.size(apparent)
            .cmp(&a.size(apparent))
            .then_with(|| a.name.cmp(&b.name))
    });
    for c in &mut node.children {
        sort_tree(c, apparent);
    }
}

/// Drop nodes deeper than `depth` (root = depth 0) to keep output small.
pub fn prune(node: &mut DirNode, depth: usize) {
    if depth == 0 {
        node.children.clear();
        return;
    }
    for c in &mut node.children {
        prune(c, depth - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options {
            one_file_system: false,
            exclude: vec![],
            top_files: 3,
            apparent: true,
        }
    }

    #[test]
    fn sums_and_top_files() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("a/b")).unwrap();
        fs::write(t.path().join("a/b/big"), vec![0u8; 50_000]).unwrap();
        fs::write(t.path().join("a/mid"), vec![0u8; 20_000]).unwrap();
        for i in 0..10 {
            fs::write(t.path().join(format!("s{i}")), vec![0u8; 100 + i]).unwrap();
        }
        let r = scan(t.path(), &opts()).unwrap();
        assert_eq!(r.root.files, 12);
        assert_eq!(r.root.dirs, 2);
        let files_logical: u64 = 50_000 + 20_000 + (0..10).map(|i| 100 + i).sum::<u64>();
        // Root logical includes directory entry sizes too.
        assert!(r.root.logical >= files_logical);
        let paths: Vec<&str> = r.largest_files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a/b/big", "a/mid", "s9"]);
        assert_eq!(r.root.children[0].name, "a");
    }

    #[test]
    fn excludes_by_name_and_path() {
        let t = tempfile::tempdir().unwrap();
        fs::create_dir_all(t.path().join("keep/node_modules")).unwrap();
        fs::write(t.path().join("keep/node_modules/x"), vec![0u8; 1000]).unwrap();
        fs::write(t.path().join("keep/a.log"), "x").unwrap();
        fs::write(t.path().join("keep/a.txt"), "x").unwrap();
        let mut o = opts();
        o.exclude = vec![
            Pattern::new("node_modules").unwrap(),
            Pattern::new("*.log").unwrap(),
        ];
        let r = scan(t.path(), &o).unwrap();
        assert_eq!(r.root.files, 1);
        assert_eq!(r.excluded, 2);
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_counted_once_and_symlinks_skipped() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("orig"), vec![1u8; 10_000]).unwrap();
        fs::hard_link(t.path().join("orig"), t.path().join("link")).unwrap();
        std::os::unix::fs::symlink("orig", t.path().join("sym")).unwrap();
        let r = scan(t.path(), &opts()).unwrap();
        assert_eq!(r.root.files, 1);
        assert_eq!(r.hardlinks_deduplicated, 1);
        assert_eq!(r.symlinks_skipped, 1);
    }

    #[test]
    fn top_merge_is_bounded() {
        let mut a: Vec<FileEntry> = (0..10)
            .map(|i| FileEntry {
                path: format!("{i}"),
                logical: i,
                allocated: i,
            })
            .collect();
        merge_top(
            &mut a,
            (10..20)
                .map(|i| FileEntry {
                    path: format!("{i}"),
                    logical: i,
                    allocated: i,
                })
                .collect(),
            5,
            true,
        );
        let mut keys: Vec<u64> = a.iter().map(|f| f.logical).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec![15, 16, 17, 18, 19]);
    }

    #[test]
    fn prune_depth() {
        let leaf = DirNode {
            name: "c".into(),
            ..Default::default()
        };
        let mid = DirNode {
            name: "b".into(),
            children: vec![leaf],
            ..Default::default()
        };
        let mut root = DirNode {
            name: "a".into(),
            children: vec![mid],
            ..Default::default()
        };
        prune(&mut root, 1);
        assert_eq!(root.children.len(), 1);
        assert!(root.children[0].children.is_empty());
    }
}
