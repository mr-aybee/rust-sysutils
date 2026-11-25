//! FileWatch command-line entry point.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use filewatch::coalesce::{Batch, ChangeKind, Raw};
use filewatch::filter::Filter;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use sys_common::glob::Pattern;

const EXIT_OK: u8 = 0;
const EXIT_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

/// How long to wait for a running command to exit after being asked to stop.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// Command line.
#[derive(Debug, Parser)]
#[command(
    name = "filewatch",
    version,
    about = "Watch files and directories; print changes or run a command on change",
    after_help = "Changed paths are written to the command's standard input, one per line.\nExample: filewatch src -i '*.rs' -- cargo test"
)]
struct Cli {
    /// Files or directories to watch (default: current directory).
    paths: Vec<PathBuf>,

    /// Only report paths matching GLOB (repeatable). Matches names, or relative paths if GLOB contains '/'.
    #[arg(short, long, value_name = "GLOB")]
    include: Vec<String>,

    /// Ignore paths matching GLOB (repeatable). A name pattern also excludes everything below a matching directory.
    #[arg(short, long, value_name = "GLOB")]
    exclude: Vec<String>,

    /// Do not ignore version-control directories (.git, .hg, .svn).
    #[arg(long)]
    no_vcs_ignore: bool,

    /// Watch only the given directories, not their subdirectories.
    #[arg(short = 'N', long)]
    no_recursive: bool,

    /// Quiet period that groups bursts of events into one batch.
    #[arg(short, long, default_value = "100ms", value_parser = parse_duration)]
    debounce: Duration,

    /// Kill a still-running command and start it again when new changes arrive (default: queue one run).
    #[arg(short, long)]
    restart: bool,

    /// Run the command once at startup, before any change.
    #[arg(short = 's', long)]
    run_on_start: bool,

    /// Exit after the first batch of changes (and its command run).
    #[arg(short = '1', long)]
    once: bool,

    /// Emit JSON Lines instead of text.
    #[arg(long)]
    json: bool,

    /// Do not print changes or command status (errors are still reported).
    #[arg(short, long)]
    quiet: bool,

    /// Command to run after each batch, given after `--`. Executed directly, never through a shell.
    #[arg(last = true, value_name = "COMMAND")]
    command: Vec<String>,
}

/// Parse `250ms`, `2s`, `1.5s`, `1m`; bare numbers are milliseconds.
fn parse_duration(s: &str) -> Result<Duration, String> {
    let t = s.trim();
    let (num, mult) = if let Some(v) = t.strip_suffix("ms") {
        (v, 0.001)
    } else if let Some(v) = t.strip_suffix('s') {
        (v, 1.0)
    } else if let Some(v) = t.strip_suffix('m') {
        (v, 60.0)
    } else {
        (t, 0.001)
    };

    let value: f64 = num.parse().map_err(|_| format!("invalid duration '{s}'"))?;
    let secs = value * mult;

    if !secs.is_finite() || !(0.0..=3600.0).contains(&secs) {
        return Err(format!("duration '{s}' must be between 0 and 1h"));
    }

    Ok(Duration::from_secs_f64(secs))
}

fn iso_now() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);

    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to (year, month, day). Algorithm by Howard Hinnant.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;

    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Translate a notify event into raw changes; `None` marks a rescan/overflow notice.
fn translate(event: Event) -> Option<Vec<Raw>> {
    if event.need_rescan() {
        return None;
    }

    let mut paths = event.paths.into_iter();

    let raws = match event.kind {
        EventKind::Create(_) => paths.map(Raw::Created).collect(),
        EventKind::Remove(_) => paths.map(Raw::Removed).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            vec![Raw::Renamed {
                from: paths.next(),
                to: paths.next(),
            }]
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => paths.map(Raw::Removed).collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => paths.map(Raw::Created).collect(),
        EventKind::Modify(ModifyKind::Name(_)) => {
            // Rename with unknown direction: decide by whether the path exists now
            paths
                .map(|p| {
                    if p.exists() {
                        Raw::Created(p)
                    } else {
                        Raw::Removed(p)
                    }
                })
                .collect()
        }
        EventKind::Modify(_) => paths.map(Raw::Modified).collect(),
        EventKind::Access(_) | EventKind::Any | EventKind::Other => Vec::new(),
    };

    Some(raws)
}

/// Resolve a batch into changes. macOS FSEvents accumulates flags per path, so event
/// kinds are unreliable there; the batch is resolved from filesystem state instead.
fn finish(
    batch: Batch,
    window_start: SystemTime,
    debounce: Duration,
) -> Vec<(PathBuf, ChangeKind)> {
    if cfg!(target_os = "macos") {
        // Allow for FSEvents delivery latency when deciding whether a file is new
        let since = window_start
            .checked_sub(debounce + Duration::from_secs(1))
            .unwrap_or(UNIX_EPOCH);

        return batch.finish_by_state(since, |p| {
            std::fs::symlink_metadata(p).ok().map(|m| m.created().ok())
        });
    }

    batch.finish()
}

/// Supervises the user command: at most one instance runs at a time.
struct Runner {
    argv: Vec<String>,
    restart: bool,
    quiet: bool,
    child: Option<Child>,
    /// Paths waiting for the next run while one is in progress.
    pending: Option<Vec<String>>,
}

impl Runner {
    fn new(argv: Vec<String>, restart: bool, quiet: bool) -> Self {
        Self {
            argv,
            restart,
            quiet,
            child: None,
            pending: None,
        }
    }

    fn enabled(&self) -> bool {
        !self.argv.is_empty()
    }

    fn running(&self) -> bool {
        self.child.is_some()
    }

    /// Request a run for `paths`: start now, queue, or restart, depending on state and policy.
    fn trigger(&mut self, paths: Vec<String>) {
        if !self.enabled() {
            return;
        }

        if self.child.is_none() {
            self.spawn(paths);
            return;
        }

        if self.restart {
            self.stop();
            self.spawn(paths);
            return;
        }

        // Queue exactly one follow-up run, merging the paths of every batch in between
        let queued = self.pending.get_or_insert_with(Vec::new);
        queued.extend(paths);
        queued.sort();
        queued.dedup();
    }

    fn spawn(&mut self, paths: Vec<String>) {
        let mut cmd = Command::new(&self.argv[0]);
        cmd.args(&self.argv[1..])
            .stdin(Stdio::piped())
            .env("FILEWATCH_CHANGED", paths.len().to_string());

        match cmd.spawn() {
            Ok(mut child) => {
                // Write the changed paths and close stdin so the command sees EOF
                if let Some(mut stdin) = child.stdin.take() {
                    let body: String = paths.iter().map(|p| format!("{p}\n")).collect();

                    // The command may exit without reading its input; that is not an error
                    let _ = stdin.write_all(body.as_bytes());
                }

                self.child = Some(child);
            }
            Err(e) => eprintln!("filewatch: cannot run '{}': {e}", self.argv[0]),
        }
    }

    /// Kill the running command and wait for it to exit (bounded by [`KILL_GRACE`]).
    fn stop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };

        let _ = child.kill();
        let deadline = Instant::now() + KILL_GRACE;

        while Instant::now() < deadline {
            if let Ok(Some(_)) = child.try_wait() {
                return;
            }

            std::thread::sleep(Duration::from_millis(10));
        }

        eprintln!("filewatch: command did not exit after kill");
    }

    /// Reap a finished command and start the queued run, if any.
    fn poll(&mut self) {
        let Some(child) = self.child.as_mut() else {
            return;
        };

        match child.try_wait() {
            Ok(Some(status)) => {
                self.child = None;

                if !self.quiet && !status.success() {
                    eprintln!("filewatch: command exited with {status}");
                }

                if let Some(paths) = self.pending.take() {
                    self.spawn(paths);
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("filewatch: cannot check command status: {e}");
                self.child = None;
            }
        }
    }

    /// Wait for the current and any queued run to finish.
    fn drain(&mut self) {
        while self.running() {
            self.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

fn emit(json: bool, path: &str, kind: Option<ChangeKind>) {
    let time = iso_now();

    let line = match (json, kind) {
        (true, Some(k)) => serde_json::json!({"time": time, "event": k, "path": path}).to_string(),
        (true, None) => {
            serde_json::json!({"time": time, "event": "overflow", "path": path}).to_string()
        }
        (false, Some(k)) => format!("{time}  {:<8} {path}", format!("{k:?}").to_uppercase()),
        (false, None) => format!(
            "{time}  OVERFLOW {path}  (events were lost; treat everything under it as changed)"
        ),
    };

    let _ = writeln!(std::io::stdout().lock(), "{line}");
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();

            return ExitCode::from(if e.use_stderr() { EXIT_USAGE } else { EXIT_OK });
        }
    };

    let compile = |globs: &[String]| {
        globs
            .iter()
            .map(|g| Pattern::new(g))
            .collect::<Result<Vec<_>, _>>()
    };

    let (include, exclude) = match (compile(&cli.include), compile(&cli.exclude)) {
        (Ok(i), Ok(e)) => (i, e),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("filewatch: {e}");

            return ExitCode::from(EXIT_USAGE);
        }
    };

    let inputs = if cli.paths.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        cli.paths.clone()
    };
    let mut roots = Vec::with_capacity(inputs.len());

    // Canonical roots let event paths (always absolute) be matched and shown relative to them
    for p in &inputs {
        match p.canonicalize() {
            Ok(c) => roots.push(c),
            Err(e) => {
                eprintln!("filewatch: cannot watch {}: {e}", p.display());

                return ExitCode::from(EXIT_FAILURE);
            }
        }
    }

    let filter = Filter {
        roots: roots.clone(),
        include,
        exclude,
        ignore_vcs: !cli.no_vcs_ignore,
    };
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("filewatch: cannot start watcher: {e}");

            return ExitCode::from(EXIT_FAILURE);
        }
    };

    let mode = if cli.no_recursive {
        RecursiveMode::NonRecursive
    } else {
        RecursiveMode::Recursive
    };

    for root in &roots {
        if let Err(e) = watcher.watch(root, mode) {
            eprintln!("filewatch: cannot watch {}: {e}", root.display());

            return ExitCode::from(EXIT_FAILURE);
        }
    }

    if !cli.quiet && !cli.json {
        eprintln!(
            "filewatch: watching {} path(s); Ctrl+C to stop",
            roots.len()
        );
    }

    let mut runner = Runner::new(cli.command.clone(), cli.restart, cli.quiet);

    if cli.run_on_start {
        runner.trigger(Vec::new());
    }

    let display = |p: &Path| {
        filter
            .relative(p)
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| p.display().to_string())
    };

    loop {
        // Block while idle; poll the command every 50 ms while one is running
        let first = if runner.running() {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(ev) => ev,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    runner.poll();
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            match rx.recv() {
                Ok(ev) => ev,
                Err(_) => break,
            }
        };

        // Collect everything that arrives within the debounce window after the first event
        let window_start = SystemTime::now();
        let deadline = Instant::now() + cli.debounce;
        let mut events = vec![first];

        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(ev) => events.push(ev),
                Err(_) => break,
            }
        }

        let mut batch = Batch::default();
        let mut overflow = false;

        for ev in events {
            match ev {
                Ok(event) => match translate(event) {
                    Some(raws) => raws.into_iter().for_each(|r| batch.push(r)),
                    None => overflow = true,
                },
                Err(e) => eprintln!("filewatch: watch error: {e}"),
            }
        }

        let mut changed: Vec<String> = Vec::new();

        for (path, kind) in finish(batch, window_start, cli.debounce) {
            // Events for a watched root itself (directory metadata) carry no useful change
            if roots.contains(&path) {
                continue;
            }

            if !filter.accepts(&path) {
                continue;
            }

            let shown = display(&path);

            if !cli.quiet {
                emit(cli.json, &shown, Some(kind));
            }

            changed.push(shown);
        }

        // An overflow means events were dropped by the OS: report it and treat the roots as changed
        if overflow {
            for root in &roots {
                let shown = root.display().to_string();
                emit(cli.json, &shown, None);
                changed.push(shown);
            }
        }

        if changed.is_empty() {
            runner.poll();
            continue;
        }

        runner.trigger(changed);

        if cli.once {
            runner.drain();

            return ExitCode::from(EXIT_OK);
        }

        runner.poll();
    }

    runner.drain();

    ExitCode::from(EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(parse_duration("2s"), Ok(Duration::from_secs(2)));
        assert_eq!(parse_duration("0"), Ok(Duration::ZERO));
        assert_eq!(parse_duration("150"), Ok(Duration::from_millis(150)));
        assert!(parse_duration("2h").is_err());
        assert!(parse_duration("-1s").is_err());
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
    }
}
