//! End-to-end tests: real filesystem events on temporary trees.
#![allow(clippy::unwrap_used)] // Test fixtures: panicking on setup failure is the desired behavior.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use predicates::prelude::*;

fn bin() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("filewatch")
}

/// Start filewatch with `args`, give the watcher time to register, run `actions`,
/// then stop it and return its stdout.
fn watch_while(args: &[&str], actions: impl FnOnce()) -> String {
    let mut child = Command::new(bin())
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(400));

    actions();

    thread::sleep(Duration::from_millis(600));
    stop(&mut child)
}

fn stop(child: &mut Child) -> String {
    child.kill().unwrap();
    child.wait().unwrap();

    let mut out = String::new();

    // Tests that discard stdout have nothing to read
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_string(&mut out).unwrap();
    }

    out
}

fn events(json: &str) -> Vec<(String, String)> {
    json.lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            (
                v["event"].as_str().unwrap().to_string(),
                v["path"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn reports_create_modify_remove_as_json() {
    let t = tempfile::tempdir().unwrap();
    fs::write(t.path().join("existing.txt"), "1").unwrap();
    fs::write(t.path().join("doomed.txt"), "1").unwrap();
    let root = t.path().to_str().unwrap().to_string();

    let out = watch_while(&[&root, "--json", "-d", "100ms"], || {
        fs::write(t.path().join("new.txt"), "x").unwrap();
        fs::write(t.path().join("existing.txt"), "2").unwrap();
        fs::remove_file(t.path().join("doomed.txt")).unwrap();
    });

    let ev = events(&out);

    assert!(ev.contains(&("created".into(), "new.txt".into())), "{out}");
    assert!(
        ev.contains(&("modified".into(), "existing.txt".into())),
        "{out}"
    );
    assert!(
        ev.contains(&("removed".into(), "doomed.txt".into())),
        "{out}"
    );
}

#[test]
fn include_exclude_and_vcs_ignore() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("target/debug")).unwrap();
    fs::create_dir_all(t.path().join(".git")).unwrap();
    fs::create_dir_all(t.path().join("src")).unwrap();
    let root = t.path().to_str().unwrap().to_string();

    let out = watch_while(&[&root, "--json", "-i", "*.rs", "-e", "target"], || {
        fs::write(t.path().join("src/lib.rs"), "x").unwrap();
        fs::write(t.path().join("src/notes.md"), "x").unwrap();
        fs::write(t.path().join("target/debug/out.rs"), "x").unwrap();
        fs::write(t.path().join(".git/HEAD.rs"), "x").unwrap();
    });

    let paths: Vec<String> = events(&out).into_iter().map(|(_, p)| p).collect();

    assert_eq!(paths, vec!["src/lib.rs".to_string()], "{out}");
}

#[test]
fn transient_files_are_coalesced_away() {
    let t = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = t.path().to_str().unwrap().to_string();

    let out = watch_while(&[&root, "--json", "-d", "300ms"], || {
        fs::write(t.path().join("scratch.tmp"), "x").unwrap();
        fs::rename(
            t.path().join("scratch.tmp"),
            outside.path().join("scratch.tmp"),
        )
        .unwrap();
        fs::write(t.path().join("draft.swp"), "x").unwrap();
        fs::rename(t.path().join("draft.swp"), t.path().join("final.txt")).unwrap();
    });

    let ev = events(&out);

    assert_eq!(
        ev,
        vec![("created".to_string(), "final.txt".to_string())],
        "{out}"
    );
}

#[test]
fn exec_receives_changed_paths_on_stdin_and_once_exits() {
    let t = tempfile::tempdir().unwrap();
    let marker = t.path().join("sub");
    fs::create_dir(&marker).unwrap();
    let root = marker.to_str().unwrap().to_string();
    let writer_dir = marker.clone();

    let writer = thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        fs::write(writer_dir.join("one.txt"), "1").unwrap();
        fs::write(writer_dir.join("two.txt"), "2").unwrap();
    });

    // `cat` echoes the paths filewatch writes to its stdin
    assert_cmd::Command::new(bin())
        .args([&root, "--once", "-q", "-d", "200ms", "--", "cat"])
        .timeout(Duration::from_secs(20))
        .assert()
        .code(0)
        .stdout(predicate::str::contains("one.txt").and(predicate::str::contains("two.txt")));

    writer.join().unwrap();
}

/// Count how many times a command ran by having it append to a log file.
fn run_count(log: &Path) -> usize {
    fs::read_to_string(log)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[cfg(unix)]
#[test]
fn queued_runs_never_overlap_and_coalesce() {
    let t = tempfile::tempdir().unwrap();
    let watched = t.path().join("w");
    fs::create_dir(&watched).unwrap();
    let log = t.path().join("runs.log");
    let script = format!("echo start >> {0}; sleep 1; echo end >> {0}", log.display());
    let root = watched.to_str().unwrap().to_string();

    let mut child = Command::new(bin())
        .args([&root, "-q", "-d", "50ms", "--", "sh", "-c", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(400));

    // Three separate batches while the first run is still in progress
    for i in 0..3 {
        fs::write(watched.join(format!("f{i}")), "x").unwrap();
        thread::sleep(Duration::from_millis(200));
    }

    thread::sleep(Duration::from_millis(3000));
    stop(&mut child);

    let lines = fs::read_to_string(&log).unwrap();

    // Exactly two runs (the first, plus one coalesced follow-up), strictly sequential
    assert_eq!(
        lines.lines().collect::<Vec<_>>(),
        vec!["start", "end", "start", "end"],
        "{lines}"
    );
}

#[cfg(unix)]
#[test]
fn restart_policy_kills_running_command() {
    let t = tempfile::tempdir().unwrap();
    let watched = t.path().join("w");
    fs::create_dir(&watched).unwrap();
    let log = t.path().join("runs.log");
    let script = format!("echo start >> {0}; sleep 5; echo end >> {0}", log.display());
    let root = watched.to_str().unwrap().to_string();

    let mut child = Command::new(bin())
        .args([&root, "-q", "-r", "-d", "50ms", "--", "sh", "-c", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(400));

    fs::write(watched.join("a"), "x").unwrap();
    thread::sleep(Duration::from_millis(500));
    fs::write(watched.join("b"), "x").unwrap();
    thread::sleep(Duration::from_millis(500));
    stop(&mut child);

    let lines = fs::read_to_string(&log).unwrap();

    // Two starts, and the first run never reached its end
    assert_eq!(run_count(&log), 2, "{lines}");
    assert!(!lines.contains("end"), "{lines}");
}

#[test]
fn usage_and_setup_errors() {
    assert_cmd::Command::new(bin())
        .args(["/definitely/not/here"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("cannot watch"));
    assert_cmd::Command::new(bin())
        .args([".", "-d", "2h"])
        .assert()
        .code(2);
    assert_cmd::Command::new(bin())
        .args([".", "-e", ""])
        .assert()
        .code(2);
}

#[test]
fn latency_is_bounded_by_debounce() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().to_str().unwrap().to_string();
    let mut child = Command::new(bin())
        .args([&root, "--json", "-d", "0ms"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    thread::sleep(Duration::from_millis(400));

    let mut stdout = child.stdout.take().unwrap();
    let start = Instant::now();
    fs::write(t.path().join("ping"), "x").unwrap();

    let mut buf = [0u8; 256];
    let n = stdout.read(&mut buf).unwrap();
    let elapsed = start.elapsed();

    child.kill().unwrap();
    child.wait().unwrap();

    assert!(String::from_utf8_lossy(&buf[..n]).contains("ping"));
    assert!(elapsed < Duration::from_secs(2), "event took {elapsed:?}");
}
