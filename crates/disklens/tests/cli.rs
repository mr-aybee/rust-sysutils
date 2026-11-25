//! End-to-end CLI tests on temporary directory trees.
#![allow(clippy::unwrap_used)] // Test fixtures: panicking on setup failure is the desired behavior.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;

fn disklens() -> Command {
    Command::cargo_bin("disklens").unwrap()
}

/// root/
///   big/        a.bin 300 KiB, b.bin 100 KiB
///   small/      1 KiB × 3
///   node_modules/ junk 500 KiB
///   top.txt     10 bytes
fn tree() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let p = t.path();
    fs::create_dir_all(p.join("big")).unwrap();
    fs::create_dir_all(p.join("small")).unwrap();
    fs::create_dir_all(p.join("node_modules")).unwrap();
    fs::write(p.join("big/a.bin"), vec![1u8; 300 * 1024]).unwrap();
    fs::write(p.join("big/b.bin"), vec![1u8; 100 * 1024]).unwrap();
    for i in 0..3 {
        fs::write(p.join(format!("small/{i}.txt")), vec![1u8; 1024]).unwrap();
    }
    fs::write(p.join("node_modules/junk"), vec![1u8; 500 * 1024]).unwrap();
    fs::write(p.join("top.txt"), "0123456789").unwrap();
    t
}

fn json(args: &[&str], root: &std::path::Path) -> serde_json::Value {
    let out = disklens()
        .arg(root)
        .args(args)
        .arg("--json")
        .assert()
        .code(0)
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn totals_and_ranking() {
    let t = tree();
    let v = json(&["-a"], t.path());
    let root = &v["root"];
    assert_eq!(root["files"], 7);
    assert_eq!(root["dirs"], 3);
    assert_eq!(
        root["logical"],
        300 * 1024 + 100 * 1024 + 3 * 1024 + 500 * 1024 + 10
    );
    let names: Vec<&str> = root["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["node_modules", "big", "small"]);
    assert_eq!(v["largest_files"][0]["path"], "node_modules/junk");
    assert_eq!(v["ranked_by"], "logical");
}

#[test]
fn exclude_and_depth() {
    let t = tree();
    let v = json(
        &["-a", "-e", "node_modules", "-e", "*.txt", "-d", "0"],
        t.path(),
    );
    assert_eq!(v["root"]["files"], 2);
    assert_eq!(v["excluded"], 5); // node_modules dir + 3 small/*.txt + top.txt
    assert!(
        v["root"].get("children").is_none(),
        "depth 0 has no children"
    );
}

#[test]
fn human_output() {
    let t = tree();
    disklens()
        .arg(t.path())
        .args(["-a", "-t", "2"])
        .assert()
        .code(0)
        .stdout(
            predicate::str::contains("├─ node_modules/")
                .and(predicate::str::contains("Largest files:"))
                .and(predicate::str::contains("500.0 KiB  node_modules/junk"))
                .and(predicate::str::contains("Total:")),
        );
}

#[test]
fn limit_summarises_rest() {
    let t = tree();
    disklens()
        .arg(t.path())
        .args(["-a", "-l", "1"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("… 2 more directories"));
}

#[test]
fn single_file_root_and_bytes() {
    let t = tree();
    disklens()
        .arg(t.path().join("top.txt"))
        .args(["-a", "-b"])
        .assert()
        .code(0)
        .stdout(predicate::str::contains("10  100.0%"));
}

#[test]
fn errors_and_usage() {
    disklens()
        .arg("/definitely/not/here")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot access"));
    disklens().args([".", "--top", "-1"]).assert().code(2);
    disklens().args([".", "-e", ""]).assert().code(2);
}

#[test]
fn parallel_and_single_thread_agree() {
    let t = tree();
    for i in 0..20 {
        let d = t.path().join(format!("deep/{i}/x/y"));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("f"), vec![0u8; i * 100]).unwrap();
    }
    let a = disklens()
        .arg(t.path())
        .args(["--json", "-j", "1", "-d", "5"])
        .assert()
        .get_output()
        .stdout
        .clone();
    let b = disklens()
        .arg(t.path())
        .args(["--json", "-j", "8", "-d", "5"])
        .assert()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        a, b,
        "output must be deterministic regardless of thread count"
    );
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_partial_failure() {
    use std::os::unix::fs::PermissionsExt;
    let t = tree();
    let locked = t.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("secret"), "x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return; // running as root
    }
    disklens()
        .arg(t.path())
        .assert()
        .code(1)
        .stdout(predicate::str::contains("1 error(s)"))
        .stderr(predicate::str::contains("locked"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn sparse_file_allocated_vs_logical() {
    let t = tempfile::tempdir().unwrap();
    let f = fs::File::create(t.path().join("sparse")).unwrap();
    f.set_len(64 * 1024 * 1024).unwrap(); // 64 MiB hole, no data blocks
    drop(f);
    let v = json(&[], t.path());
    assert_eq!(v["largest_files"][0]["logical"], 64 * 1024 * 1024);
    assert!(
        v["largest_files"][0]["allocated"].as_u64().unwrap() < 1024 * 1024,
        "sparse file should allocate almost nothing"
    );
}
