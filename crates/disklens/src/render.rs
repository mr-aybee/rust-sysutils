//! Human-readable rendering.

use crate::scan::{DirNode, ScanResult};

/// Format bytes with binary units (`1.5 GiB`), or raw bytes.
pub fn size(bytes: u64, raw: bool) -> String {
    if raw {
        return bytes.to_string();
    }

    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut v = bytes as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }

    if u == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

fn group(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);

    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }

        out.push(c);
    }

    out
}

pub struct View {
    pub depth: usize,
    pub limit: usize,
    pub apparent: bool,
    pub raw: bool,
}

pub fn render(r: &ScanResult, v: &View) -> String {
    let total = r.root.size(v.apparent).max(1);
    let mut out = format!(
        "{:>11}  {:>6}  {:>10}  PATH\n",
        if v.apparent { "APPARENT" } else { "ON DISK" },
        "%",
        "FILES"
    );
    line(&mut out, &r.root, total, v, "", true, 0);

    if !r.largest_files.is_empty() {
        out.push_str("\nLargest files:\n");

        for f in &r.largest_files {
            let s = if v.apparent { f.logical } else { f.allocated };
            out.push_str(&format!("{:>11}  {}\n", size(s, v.raw), f.path));
        }
    }

    let alloc = if r.allocated_supported {
        size(r.root.allocated, v.raw)
    } else {
        "n/a".to_string()
    };

    out.push_str(&format!(
        "\nTotal: {} apparent, {} on disk, {} files, {} directories",
        size(r.root.logical, v.raw),
        alloc,
        group(r.root.files),
        group(r.root.dirs)
    ));
    let mut notes = Vec::new();

    if r.hardlinks_deduplicated > 0 {
        notes.push(format!(
            "{} hard link(s) counted once",
            group(r.hardlinks_deduplicated)
        ));
    }

    if r.symlinks_skipped > 0 {
        notes.push(format!(
            "{} symlink(s) counted but not followed",
            group(r.symlinks_skipped)
        ));
    }

    if r.excluded > 0 {
        notes.push(format!("{} excluded", group(r.excluded)));
    }

    if r.errors > 0 {
        notes.push(format!("{} error(s)", group(r.errors)));
    }

    if !notes.is_empty() {
        out.push_str(&format!("\n       {}", notes.join(", ")));
    }

    out.push('\n');
    out
}

fn line(
    out: &mut String,
    n: &DirNode,
    total: u64,
    v: &View,
    prefix: &str,
    last: bool,
    depth: usize,
) {
    let s = n.size(v.apparent);
    let pct = s as f64 * 100.0 / total as f64;

    let branch = if depth == 0 {
        String::new()
    } else {
        format!("{prefix}{}", if last { "└─ " } else { "├─ " })
    };

    let label = if depth == 0 {
        n.name.clone()
    } else {
        format!("{}/", n.name)
    };

    out.push_str(&format!(
        "{:>11}  {:>5.1}%  {:>10}  {branch}{label}\n",
        size(s, v.raw),
        pct,
        group(n.files)
    ));

    if depth >= v.depth {
        return;
    }

    let child_prefix = if depth == 0 {
        String::new()
    } else {
        format!("{prefix}{}", if last { "   " } else { "│  " })
    };

    let shown = n.children.len().min(v.limit);
    for (i, c) in n.children.iter().take(shown).enumerate() {
        let is_last = i + 1 == shown && n.children.len() <= v.limit;
        line(out, c, total, v, &child_prefix, is_last, depth + 1);
    }

    if n.children.len() > v.limit {
        let rest = &n.children[v.limit..];
        let rest_size: u64 = rest.iter().map(|c| c.size(v.apparent)).sum();
        out.push_str(&format!(
            "{:>11}  {:>5.1}%  {:>10}  {child_prefix}└─ … {} more director{}\n",
            size(rest_size, v.raw),
            rest_size as f64 * 100.0 / total as f64,
            group(rest.iter().map(|c| c.files).sum()),
            rest.len(),
            if rest.len() == 1 { "y" } else { "ies" }
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(size(0, false), "0 B");
        assert_eq!(size(1023, false), "1023 B");
        assert_eq!(size(1536, false), "1.5 KiB");
        assert_eq!(size(5 * 1024 * 1024 * 1024, false), "5.0 GiB");
        assert_eq!(size(1536, true), "1536");
    }

    #[test]
    fn grouping() {
        assert_eq!(group(0), "0");
        assert_eq!(group(1234), "1,234");
        assert_eq!(group(1_234_567), "1,234,567");
    }
}
