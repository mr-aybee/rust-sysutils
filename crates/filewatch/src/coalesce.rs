//! Coalescing raw filesystem events into one change per path per batch.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

/// What happened to a path within one batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// The path did not exist before the batch and exists after it.
    Created,
    /// The path existed before and after the batch, and its content or metadata changed.
    Modified,
    /// The path existed before the batch and does not exist after it.
    Removed,
}

/// A raw event, reduced to what coalescing needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raw {
    /// A path was created.
    Created(PathBuf),
    /// A path's content or metadata changed.
    Modified(PathBuf),
    /// A path was removed.
    Removed(PathBuf),
    /// A path was renamed; either side may be unknown on some platforms.
    Renamed {
        /// Old path, if reported.
        from: Option<PathBuf>,
        /// New path, if reported.
        to: Option<PathBuf>,
    },
}

/// First and last observed state of a path within a batch.
#[derive(Debug, Clone, Copy)]
struct Track {
    existed_before: bool,
    exists_after: bool,
    touched: bool,
}

/// Accumulates raw events and produces one change per path.
#[derive(Debug, Default)]
pub struct Batch {
    paths: BTreeMap<PathBuf, Track>,
}

impl Batch {
    /// Whether no events have been recorded.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    fn record(&mut self, path: PathBuf, existed_before: bool, exists_after: bool) {
        let t = self.paths.entry(path).or_insert(Track {
            existed_before,
            exists_after,
            touched: false,
        });
        t.exists_after = exists_after;
        t.touched = true;
    }

    /// Add one raw event.
    pub fn push(&mut self, raw: Raw) {
        match raw {
            Raw::Created(p) => self.record(p, false, true),
            Raw::Modified(p) => self.record(p, true, true),
            Raw::Removed(p) => self.record(p, true, false),
            Raw::Renamed { from, to } => {
                if let Some(f) = from {
                    self.record(f, true, false);
                }

                if let Some(t) = to {
                    self.record(t, false, true);
                }
            }
        }
    }

    /// Final changes, sorted by path. A path created and removed within the same
    /// batch (e.g. an editor's temporary file) produces no change.
    pub fn finish(self) -> Vec<(PathBuf, ChangeKind)> {
        self.paths
            .into_iter()
            .filter(|(_, t)| t.touched)
            .filter_map(|(p, t)| {
                let kind = match (t.existed_before, t.exists_after) {
                    (false, true) => ChangeKind::Created,
                    (true, true) => ChangeKind::Modified,
                    (true, false) => ChangeKind::Removed,
                    (false, false) => return None,
                };

                Some((p, kind))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn run(events: Vec<Raw>) -> Vec<(PathBuf, ChangeKind)> {
        let mut b = Batch::default();

        for e in events {
            b.push(e);
        }

        b.finish()
    }

    #[test]
    fn create_then_modify_is_created() {
        assert_eq!(
            run(vec![Raw::Created(p("a")), Raw::Modified(p("a"))]),
            vec![(p("a"), ChangeKind::Created)]
        );
    }

    #[test]
    fn transient_file_vanishes() {
        assert!(
            run(vec![
                Raw::Created(p("tmp")),
                Raw::Modified(p("tmp")),
                Raw::Removed(p("tmp"))
            ])
            .is_empty()
        );
    }

    #[test]
    fn atomic_save_reports_only_the_target() {
        // Editor pattern: write a temp file, then rename it over the original. The temp
        // file disappears from the batch; the target is reported as created because the
        // OS reports a new file at that path.
        let changes = run(vec![
            Raw::Created(p("a.swp")),
            Raw::Renamed {
                from: Some(p("a.swp")),
                to: Some(p("a")),
            },
        ]);

        assert_eq!(changes, vec![(p("a"), ChangeKind::Created)]);
    }

    #[test]
    fn remove_then_recreate_is_modified() {
        assert_eq!(
            run(vec![Raw::Removed(p("a")), Raw::Created(p("a"))]),
            vec![(p("a"), ChangeKind::Modified)]
        );
    }

    #[test]
    fn rename_reports_both_sides() {
        let changes = run(vec![Raw::Renamed {
            from: Some(p("old")),
            to: Some(p("new")),
        }]);

        assert_eq!(
            changes,
            vec![
                (p("new"), ChangeKind::Created),
                (p("old"), ChangeKind::Removed)
            ]
        );
    }
}
