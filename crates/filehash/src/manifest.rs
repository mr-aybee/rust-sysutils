//! Manifest formats compatible with GNU coreutils (`sha256sum`, `b3sum`) and BSD tags.
//!
//! GNU:  `<hex>  <path>` (text) or `<hex> *<path>` (binary marker, ignored).
//!       Paths containing `\` or newline are written with a leading `\` and escaped.
//! BSD:  `SHA256 (<path>) = <hex>`

use crate::algo::Algorithm;

/// One manifest line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File path as written in the manifest (unescaped).
    pub path: String,
    /// Expected digest, lower-case hex.
    pub hex: String,
    /// Algorithm named by the line itself (BSD tag format only).
    pub algorithm: Option<Algorithm>,
}

fn escape(path: &str) -> (bool, String) {
    if !path.contains(['\\', '\n', '\r']) {
        return (false, path.to_string());
    }

    let mut out = String::with_capacity(path.len() + 4);

    for c in path.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }

    (true, out)
}

fn unescape(s: &str) -> Result<String, String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }

        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            other => {
                return Err(format!(
                    "invalid escape '\\{}'",
                    other.map(String::from).unwrap_or_default()
                ));
            }
        }
    }

    Ok(out)
}

/// Format one manifest line (without trailing newline).
pub fn format_line(algo: Algorithm, hex: &str, path: &str, bsd_tag: bool) -> String {
    let (escaped, p) = escape(path);
    let prefix = if escaped { "\\" } else { "" };

    if bsd_tag {
        format!("{prefix}{} ({p}) = {hex}", algo.tag())
    } else {
        format!("{prefix}{hex}  {p}")
    }
}

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Parse one non-empty, non-comment line.
pub fn parse_line(line: &str) -> Result<Entry, String> {
    let (escaped, line) = match line.strip_prefix('\\') {
        Some(rest) => (true, rest),
        None => (false, line),
    };

    let unesc = |p: &str| {
        if escaped {
            unescape(p)
        } else {
            Ok(p.to_string())
        }
    };

    // BSD tag: ALGO (path) = hex
    if let Some((head, hex)) = line.rsplit_once(") = ")
        && let Some((tag, path)) = head.split_once(" (")
        && let Some(algo) = Algorithm::from_tag(tag)
    {
        if !is_hex(hex) || hex.len() != algo.hex_len() {
            return Err(format!("invalid {tag} digest"));
        }
        return Ok(Entry {
            path: unesc(path)?,
            hex: hex.to_ascii_lowercase(),
            algorithm: Some(algo),
        });
    }

    // GNU: hex, space, (space | '*'), path
    let (hex, rest) = line.split_once(' ').ok_or("expected '<hex>  <path>'")?;
    if !is_hex(hex) {
        return Err("digest is not hexadecimal".into());
    }

    let path = rest
        .strip_prefix(' ')
        .or_else(|| rest.strip_prefix('*'))
        .ok_or("expected two spaces or ' *' after the digest")?;

    if path.is_empty() {
        return Err("missing path".into());
    }

    Ok(Entry {
        path: unesc(path)?,
        hex: hex.to_ascii_lowercase(),
        algorithm: None,
    })
}

/// Parse a whole manifest. Blank lines and lines starting with `#` are ignored.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();

    for (i, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }

        out.push(parse_line(line).map_err(|e| format!("line {}: {e}", i + 1))?);
    }

    if out.is_empty() {
        return Err("manifest contains no entries".into());
    }

    Ok(out)
}

/// Choose the algorithm for GNU-format entries: explicit flag, then file name, then digest length.
pub fn infer_algorithm(
    explicit: Option<Algorithm>,
    manifest_name: &str,
    entries: &[Entry],
) -> Result<Algorithm, String> {
    if let Some(a) = explicit {
        return Ok(a);
    }

    let name = manifest_name.to_ascii_lowercase();
    if name.contains("b3") || name.contains("blake3") {
        return Ok(Algorithm::Blake3);
    }

    if name.contains("sha512") {
        return Ok(Algorithm::Sha512);
    }

    if name.contains("sha256") {
        return Ok(Algorithm::Sha256);
    }

    let lens: std::collections::BTreeSet<usize> = entries
        .iter()
        .filter(|e| e.algorithm.is_none())
        .map(|e| e.hex.len())
        .collect();

    match lens.into_iter().collect::<Vec<_>>().as_slice() {
        [] => Ok(Algorithm::Sha256), // all entries are tagged; value unused
        [128] => Ok(Algorithm::Sha512),
        [64] => Err("64-hex-digit digests could be SHA-256 or BLAKE3; pass --algorithm".into()),
        other => Err(format!(
            "unrecognised digest length(s) {other:?}; pass --algorithm"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn gnu_roundtrip_and_binary_marker() {
        let line = format_line(Algorithm::Sha256, H, "dir/a file.txt", false);
        assert_eq!(line, format!("{H}  dir/a file.txt"));
        assert_eq!(parse_line(&line).unwrap().path, "dir/a file.txt");
        assert_eq!(
            parse_line(&format!("{H} *bin.dat")).unwrap().path,
            "bin.dat"
        );
    }

    #[test]
    fn escaping() {
        let line = format_line(Algorithm::Sha256, H, "a\\b\nc", false);
        assert_eq!(line, format!("\\{H}  a\\\\b\\nc"));
        assert_eq!(parse_line(&line).unwrap().path, "a\\b\nc");
    }

    #[test]
    fn bsd_tags() {
        let line = format_line(Algorithm::Blake3, H, "x (1).txt", true);
        assert_eq!(line, format!("BLAKE3 (x (1).txt) = {H}"));
        let e = parse_line(&line).unwrap();
        assert_eq!(
            (e.path.as_str(), e.algorithm),
            ("x (1).txt", Some(Algorithm::Blake3))
        );
        assert!(parse_line("SHA256 (x) = abc").is_err());
    }

    #[test]
    fn rejects_bad_lines() {
        for bad in [
            "",
            "zz  x",
            &format!("{H} x"),
            &format!("{H}  "),
            "\\abc  \\q",
        ] {
            assert!(parse_line(bad).is_err(), "{bad:?}");
        }

        assert!(parse("# only comment\n").is_err());
    }

    #[test]
    fn inference() {
        let e = parse(&format!("{H}  a\n")).unwrap();
        assert_eq!(
            infer_algorithm(None, "SHA256SUMS", &e),
            Ok(Algorithm::Sha256)
        );
        assert_eq!(
            infer_algorithm(None, "release.b3", &e),
            Ok(Algorithm::Blake3)
        );
        assert!(infer_algorithm(None, "sums.txt", &e).is_err());
        assert_eq!(
            infer_algorithm(Some(Algorithm::Blake3), "SHA256SUMS", &e),
            Ok(Algorithm::Blake3)
        );
    }

    proptest::proptest! {
        #[test]
        fn roundtrip_any_path(path in "\\PC{1,40}", bsd: bool) {
            let line = format_line(Algorithm::Sha256, H, &path, bsd);
            let e = parse_line(&line).unwrap();
            proptest::prop_assert_eq!(e.path, path);
            proptest::prop_assert_eq!(e.hex, H);
        }

        #[test]
        fn parse_never_panics(s in "\\PC{0,120}") {
            let _ = parse_line(&s);
        }
    }
}
