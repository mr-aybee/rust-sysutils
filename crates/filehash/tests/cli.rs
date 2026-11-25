//! End-to-end CLI tests on temporary directory trees.
#![allow(clippy::unwrap_used)] // Test fixtures: panicking on setup failure is the desired behavior.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;

fn filehash(dir: &Path) -> Command {
    let mut c = Command::cargo_bin("filehash").unwrap();
    c.current_dir(dir);
    c
}

/// d/a.txt = "hello\n", d/sub/ü b.txt = 5000 × 'x', d/.hidden
fn tree() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("d/sub")).unwrap();
    fs::write(t.path().join("d/a.txt"), "hello\n").unwrap();
    fs::write(t.path().join("d/sub/ü b.txt"), "x".repeat(5000)).unwrap();
    fs::write(t.path().join("d/.hidden"), "secret").unwrap();
    t
}

const HELLO_SHA256: &str = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

#[test]
fn hash_is_sorted_deterministic_and_skips_hidden() {
    let t = tree();
    let out = filehash(t.path())
        .args(["hash", "d", "-j", "4"])
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(out).unwrap();
    assert_eq!(text.lines().count(), 2);
    assert_eq!(
        text.lines().next().unwrap(),
        format!("{HELLO_SHA256}  d/a.txt")
    );
    assert!(text.lines().nth(1).unwrap().ends_with("  d/sub/ü b.txt"));
    let again = filehash(t.path())
        .args(["hash", "d", "-j", "1"])
        .assert()
        .get_output()
        .stdout
        .clone();
    assert_eq!(text.as_bytes(), again.as_slice());
    filehash(t.path())
        .args(["hash", "d", "--hidden"])
        .assert()
        .stdout(predicate::str::contains("d/.hidden"));
}

#[test]
fn roundtrip_all_algorithms_and_formats() {
    let t = tree();

    for algo in ["sha256", "sha512", "blake3"] {
        for tag in [false, true] {
            let name = format!("{algo}{}.sums", if tag { "-tag" } else { "" });
            let mut cmd = filehash(t.path());
            cmd.args(["hash", "d", "-a", algo, "-o", &name]);

            if tag {
                cmd.arg("--tag");
            }

            cmd.assert().code(0);

            // Untagged 64-hex manifests need the algorithm unless the name says it.
            filehash(t.path())
                .args(["verify", &name])
                .assert()
                .code(0)
                .stdout(predicate::str::contains("2 ok"));
        }
    }
}

#[test]
fn ambiguous_manifest_needs_algorithm() {
    let t = tree();
    filehash(t.path())
        .args(["hash", "d", "-a", "blake3", "-o", "sums.txt"])
        .assert()
        .code(0);
    filehash(t.path())
        .args(["verify", "sums.txt"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--algorithm"));
    filehash(t.path())
        .args(["verify", "sums.txt", "-a", "blake3"])
        .assert()
        .code(0);
    filehash(t.path())
        .args(["verify", "sums.txt", "-a", "sha256"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("FAILED"));
}

#[test]
fn detects_tampering_missing_and_extra() {
    let t = tree();
    filehash(t.path())
        .args(["hash", "d", "-o", "SHA256SUMS"])
        .assert()
        .code(0);
    fs::write(t.path().join("d/a.txt"), "tampered").unwrap();
    fs::remove_file(t.path().join("d/sub/ü b.txt")).unwrap();
    fs::write(t.path().join("d/new.txt"), "new").unwrap();
    let out = filehash(t.path())
        .args(["verify", "SHA256SUMS", "--report-extra", "--json"])
        .assert()
        .code(1)
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(v["failed"], 1);
    assert_eq!(v["missing"], 1);
    let extras: Vec<&str> = v["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["status"] == "extra")
        .map(|r| r["path"].as_str().unwrap())
        .collect();
    assert!(extras.contains(&"d/new.txt"), "{extras:?}");
    assert!(
        !extras.contains(&"SHA256SUMS"),
        "manifest itself must not be reported"
    );
}

#[test]
fn verify_with_root_and_quiet() {
    let t = tree();
    let manifest = format!("{HELLO_SHA256}  a.txt\n");
    filehash(t.path())
        .args(["verify", "-", "-a", "sha256", "--root", "d", "-q"])
        .write_stdin(manifest)
        .assert()
        .code(0)
        .stdout(predicate::str::starts_with("--- 1 checked: 1 ok"));
}

#[test]
fn interoperates_with_gnu_binary_marker_and_crlf() {
    let t = tree();
    let manifest = format!("# comment\r\n{HELLO_SHA256} *d/a.txt\r\n");
    fs::write(t.path().join("SHA256SUMS"), manifest).unwrap();
    filehash(t.path())
        .args(["verify", "SHA256SUMS"])
        .assert()
        .code(0);
}

#[test]
fn stdin_hashing() {
    let t = tree();
    filehash(t.path())
        .args(["hash", "-"])
        .write_stdin("abc")
        .assert()
        .code(0)
        .stdout("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  -\n");
}

#[test]
fn errors_and_usage() {
    let t = tree();
    filehash(t.path())
        .args(["hash", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("nope"));
    filehash(t.path())
        .args(["hash", "d", "-a", "md5"])
        .assert()
        .code(2);
    filehash(t.path())
        .args(["verify", "missing.sums"])
        .assert()
        .code(2);
    fs::write(t.path().join("bad.sums"), "not a manifest\n").unwrap();
    filehash(t.path())
        .args(["verify", "bad.sums", "-a", "sha256"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("line 1"));
    filehash(t.path()).assert().code(2);
}

#[test]
fn output_is_atomic_and_not_partial_on_error() {
    let t = tree();

    // Output into a non-existent directory fails cleanly, leaving nothing behind.
    filehash(t.path())
        .args(["hash", "d", "-o", "no/such/dir/sums"])
        .assert()
        .code(1);
    assert!(!t.path().join("no").exists());
    filehash(t.path())
        .args(["hash", "d", "-o", "out.sums"])
        .assert()
        .code(0);
    let leftovers: Vec<_> = fs::read_dir(t.path())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        .collect();
    assert!(leftovers.is_empty());
}

#[cfg(unix)]
#[test]
fn symlinks_not_followed_by_default_and_cycles_safe() {
    let t = tree();
    std::os::unix::fs::symlink("a.txt", t.path().join("d/link")).unwrap();
    std::os::unix::fs::symlink(".", t.path().join("d/sub/loop")).unwrap();
    filehash(t.path())
        .args(["hash", "d"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("d/link").not())
        .stderr(predicate::str::contains("2 symlink(s) skipped"));

    // Following links must terminate despite the cycle and include the linked file.
    filehash(t.path())
        .args(["hash", "d", "-L"])
        .timeout(std::time::Duration::from_secs(20))
        .assert()
        .code(0)
        .stdout(predicate::str::contains("d/link"));
}

#[cfg(unix)]
#[test]
fn unreadable_file_is_an_error_not_a_match() {
    use std::os::unix::fs::PermissionsExt;
    let t = tree();
    let p = t.path().join("d/a.txt");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();

    if fs::read(&p).is_ok() {
        return; // running as root: permissions are not enforced
    }

    filehash(t.path())
        .args(["hash", "d"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("d/a.txt"));
    let manifest = format!("{HELLO_SHA256}  d/a.txt\n");
    filehash(t.path())
        .args(["verify", "-", "-a", "sha256"])
        .write_stdin(manifest)
        .assert()
        .code(1)
        .stdout(predicate::str::contains("ERROR"));
}
