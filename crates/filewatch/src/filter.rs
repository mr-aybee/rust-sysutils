//! Include/exclude filtering of changed paths.

use std::path::{Component, Path, PathBuf};

use sys_common::glob::Pattern;

/// Directory names ignored unless explicitly included.
pub const DEFAULT_IGNORED_DIRS: &[&str] = &[".git", ".hg", ".svn"];

/// Decides which changed paths are reported.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    /// Watched roots; changed paths are matched relative to the root containing them.
    pub roots: Vec<PathBuf>,
    /// If non-empty, a path must match at least one of these.
    pub include: Vec<Pattern>,
    /// A path matching any of these is ignored.
    pub exclude: Vec<Pattern>,
    /// Skip [`DEFAULT_IGNORED_DIRS`] (version-control metadata).
    pub ignore_vcs: bool,
}

impl Filter {
    /// Path relative to its watched root, `/`-separated, or `None` if outside every root.
    pub fn relative(&self, path: &Path) -> Option<String> {
        let root = self
            .roots
            .iter()
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.as_os_str().len())?;
        let rel = path.strip_prefix(root).ok()?;
        let parts: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();

        Some(parts.join("/"))
    }

    /// Whether a change to `path` should be reported.
    pub fn accepts(&self, path: &Path) -> bool {
        let Some(rel) = self.relative(path) else {
            return false;
        };

        let name = rel.rsplit('/').next().unwrap_or_default();

        // Version-control metadata changes constantly and is almost never interesting
        if self.ignore_vcs
            && rel
                .split('/')
                .any(|seg| DEFAULT_IGNORED_DIRS.contains(&seg))
        {
            return false;
        }

        // A pattern without '/' matches any path component, so excluding `target` skips everything below it
        let segments: Vec<&str> = rel.split('/').collect();
        let matches = |p: &Pattern| {
            if p.path_match {
                p.matches(name, &rel) || prefixes(&segments).any(|prefix| p.matches("", &prefix))
            } else {
                segments.iter().any(|seg| p.matches(seg, &rel))
            }
        };

        if self.exclude.iter().any(matches) {
            return false;
        }

        self.include.is_empty() || self.include.iter().any(|p| p.matches(name, &rel))
    }
}

/// `a`, `a/b`, `a/b/c` for segments `[a, b, c]`.
fn prefixes<'a>(segments: &'a [&'a str]) -> impl Iterator<Item = String> + 'a {
    (1..=segments.len()).map(move |n| segments[..n].join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(include: &[&str], exclude: &[&str]) -> Filter {
        Filter {
            roots: vec![PathBuf::from("/w")],
            include: include.iter().map(|p| Pattern::new(p).unwrap()).collect(),
            exclude: exclude.iter().map(|p| Pattern::new(p).unwrap()).collect(),
            ignore_vcs: true,
        }
    }

    #[test]
    fn relative_paths() {
        let f = filter(&[], &[]);

        assert_eq!(
            f.relative(Path::new("/w/src/a.rs")).as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(f.relative(Path::new("/other/a.rs")), None);
    }

    #[test]
    fn excludes_by_component_and_path() {
        let f = filter(&[], &["target", "docs/*.tmp"]);

        assert!(!f.accepts(Path::new("/w/target/debug/x")));
        assert!(!f.accepts(Path::new("/w/docs/a.tmp")));
        assert!(f.accepts(Path::new("/w/docs/sub/a.tmp")));
        assert!(f.accepts(Path::new("/w/src/target.rs")));
    }

    #[test]
    fn includes_by_name() {
        let f = filter(&["*.rs", "Cargo.toml"], &[]);

        assert!(f.accepts(Path::new("/w/src/main.rs")));
        assert!(f.accepts(Path::new("/w/Cargo.toml")));
        assert!(!f.accepts(Path::new("/w/README.md")));
    }

    #[test]
    fn vcs_ignored_by_default() {
        let f = filter(&[], &[]);

        assert!(!f.accepts(Path::new("/w/.git/index")));

        let mut g = f.clone();
        g.ignore_vcs = false;

        assert!(g.accepts(Path::new("/w/.git/index")));
    }

    #[test]
    fn longest_root_wins() {
        let mut f = filter(&[], &["inner"]);
        f.roots.push(PathBuf::from("/w/inner"));

        // Relative to /w/inner the path is `x`, which the exclude does not match
        assert!(f.accepts(Path::new("/w/inner/x")));
    }
}
