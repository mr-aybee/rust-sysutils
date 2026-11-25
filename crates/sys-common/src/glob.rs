//! Minimal glob matching for `--exclude`: `*` (any run except `/`), `**` (any run
//! including `/`), `?` (one character except `/`). Patterns without `/` match the
//! entry name; patterns with `/` match the path relative to the scan root.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pattern {
    pat: Vec<char>,
    pub path_match: bool,
}

impl Pattern {
    pub fn new(p: &str) -> Result<Self, String> {
        if p.is_empty() {
            return Err("empty exclude pattern".into());
        }

        let trimmed = p.trim_end_matches('/');
        Ok(Self {
            pat: trimmed.chars().collect(),
            path_match: trimmed.contains('/'),
        })
    }

    /// `name`: entry file name; `rel`: path relative to the root using `/`.
    pub fn matches(&self, name: &str, rel: &str) -> bool {
        let text: Vec<char> = if self.path_match {
            rel.chars().collect()
        } else {
            name.chars().collect()
        };

        glob_match(&self.pat, &text)
    }
}

fn glob_match(p: &[char], t: &[char]) -> bool {
    // Iterative backtracking matcher; `**` may cross '/', `*` may not.
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<(usize, usize, bool)> = None; // (pattern idx after star, text idx, crosses_slash)

    while ti < t.len() {
        if pi < p.len() && p[pi] == '*' {
            let double = pi + 1 < p.len() && p[pi + 1] == '*';
            pi += if double { 2 } else { 1 };

            // `**/` also matches zero directories.
            if double && pi < p.len() && p[pi] == '/' {
                pi += 1;
            }

            star = Some((pi, ti, double));

            continue;
        }

        if pi < p.len() && (p[pi] == t[ti] || (p[pi] == '?' && t[ti] != '/')) {
            pi += 1;
            ti += 1;

            continue;
        }

        match star {
            Some((sp, st, crosses)) if crosses || t[st] != '/' => {
                star = Some((sp, st + 1, crosses));
                pi = sp;
                ti = st + 1;
            }
            _ => return false,
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }

    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, name: &str, rel: &str) -> bool {
        Pattern::new(p).unwrap().matches(name, rel)
    }

    #[test]
    fn name_patterns() {
        assert!(m("node_modules", "node_modules", "a/node_modules"));
        assert!(m("*.log", "x.log", "a/x.log"));
        assert!(!m("*.log", "x.log.gz", "a/x.log.gz"));
        assert!(m("?.txt", "a.txt", "a.txt"));
        assert!(m(".git", ".git", ".git"));
        assert!(!m("target", "targets", "targets"));
    }

    #[test]
    fn path_patterns() {
        assert!(m("build/out", "out", "build/out"));
        assert!(!m("build/out", "out", "x/build/out"));
        assert!(m("**/cache", "cache", "x/y/cache"));
        assert!(m("**/cache", "cache", "cache"));
        assert!(m("a/*/c", "c", "a/b/c"));
        assert!(!m("a/*/c", "c", "a/b/x/c"));
        assert!(m("a/**/c", "c", "a/b/x/c"));

        // A trailing slash is ignored: "logs/" matches entries named logs.
        assert!(m("logs/", "logs", "x/logs"));
    }

    proptest::proptest! {
        #[test]
        fn literal_matches_itself(s in "[a-z0-9._-]{1,20}") {
            proptest::prop_assert!(m(&s, &s, &s));
        }

        #[test]
        fn star_matches_any_name(s in "[a-zA-Z0-9._ -]{0,30}") {
            proptest::prop_assert!(m("*", &s, &s));
        }

        #[test]
        fn never_panics(p in "[*?/a-c]{1,8}", t in "[/a-c]{0,12}") {
            let _ = m(&p, &t, &t);
        }
    }
}
