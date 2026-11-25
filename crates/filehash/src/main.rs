//! FileHash command-line entry point.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use filehash::algo::Algorithm;
use filehash::{exit, manifest, pool, walk};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "filehash",
    version,
    about = "Parallel file hashing and integrity verification",
    propagate_version = true
)]
struct Cli {
    /// Emit a JSON document instead of manifest lines / status lines.
    #[arg(long, global = true)]
    json: bool,
    /// Only report problems (verify) / suppress the summary (hash).
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Worker threads (default: available CPUs, max 64).
    #[arg(short = 'j', long, global = true, value_parser = clap::value_parser!(u16).range(1..=256))]
    jobs: Option<u16>,
    /// Never memory-map files; always stream through fixed-size buffers.
    /// (By default BLAKE3 maps large files when there are fewer files than workers.)
    #[arg(long, global = true)]
    no_mmap: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Hash files and directories (recursively) and print a manifest.
    Hash {
        /// Files or directories; `-` reads standard input.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// sha256, sha512 or blake3.
        #[arg(short, long, default_value = "sha256")]
        algorithm: Algorithm,
        /// Write the manifest to FILE atomically instead of stdout.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
        /// BSD-style tagged lines: `SHA256 (path) = hex`.
        #[arg(long)]
        tag: bool,
        /// Follow symbolic links while walking directories.
        #[arg(short = 'L', long)]
        follow: bool,
        /// Include hidden files and directories (names starting with '.').
        #[arg(long)]
        hidden: bool,
    },
    /// Verify files against a manifest (sha256sum/b3sum or BSD tag format).
    Verify {
        /// Manifest file; `-` reads standard input.
        manifest: PathBuf,
        /// Algorithm for untagged lines (default: inferred from the manifest name).
        #[arg(short, long)]
        algorithm: Option<Algorithm>,
        /// Resolve relative paths against DIR (default: current directory, like sha256sum -c).
        #[arg(long, value_name = "DIR")]
        root: Option<PathBuf>,
        /// Also report files under the root that are not listed in the manifest.
        #[arg(long)]
        report_extra: bool,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() {
                exit::USAGE
            } else {
                exit::OK
            });
        }
    };

    let jobs = cli.jobs.map(usize::from).unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(64)
    });

    let code = match cli.command {
        Command::Hash {
            paths,
            algorithm,
            output,
            tag,
            follow,
            hidden,
        } => cmd_hash(
            &paths,
            algorithm,
            output.as_deref(),
            tag,
            follow,
            hidden,
            jobs,
            !cli.no_mmap,
            cli.json,
            cli.quiet,
        ),
        Command::Verify {
            manifest,
            algorithm,
            root,
            report_extra,
        } => cmd_verify(
            &manifest,
            algorithm,
            root.as_deref(),
            report_extra,
            jobs,
            !cli.no_mmap,
            cli.json,
            cli.quiet,
        ),
    };

    ExitCode::from(code)
}

#[derive(Serialize)]
struct HashedJson<'a> {
    path: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hash: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

fn hash_stdin(algo: Algorithm) -> io::Result<(String, u64)> {
    let mut h = algo.hasher();
    let mut buf = vec![0u8; pool::DEFAULT_BUFFER];
    let mut stdin = io::stdin().lock();
    let mut total = 0u64;

    loop {
        let n = match stdin.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };

        h.update(&buf[..n]);
        total += n as u64;
    }

    Ok((h.finalize_hex(), total))
}

/// Write to a temporary file in the destination directory, then rename over the target.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("output path has no file name"))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }

    result
}

#[allow(clippy::too_many_arguments)]
fn cmd_hash(
    paths: &[PathBuf],
    algo: Algorithm,
    output: Option<&Path>,
    tag: bool,
    follow: bool,
    hidden: bool,
    jobs: usize,
    mmap: bool,
    json: bool,
    quiet: bool,
) -> u8 {
    let stdin_requested = paths.iter().any(|p| p.as_os_str() == "-");
    let fs_inputs: Vec<PathBuf> = paths
        .iter()
        .filter(|p| p.as_os_str() != "-")
        .cloned()
        .collect();
    let walked = walk::collect(&fs_inputs, follow, hidden);
    let files: Vec<&Path> = walked.files.iter().map(|f| f.path.as_path()).collect();
    let mut results = pool::hash_all(&files, algo, jobs, pool::DEFAULT_BUFFER, mmap, &|_, _| {});
    let mut displays: Vec<String> = walked.files.iter().map(|f| f.display.clone()).collect();

    if stdin_requested {
        displays.push("-".into());
        results.push(hash_stdin(algo).map_err(|e| e.to_string()));
    }

    let mut failed = walked.errors.len();

    for (p, e) in &walked.errors {
        eprintln!("filehash: {p}: {e}");
    }

    let mut text = String::new();
    let mut json_items = Vec::with_capacity(results.len());

    for (display, r) in displays.iter().zip(&results) {
        match r {
            Ok((hex, size)) => {
                if json {
                    json_items.push(HashedJson {
                        path: display,
                        hash: Some(hex),
                        size: Some(*size),
                        error: None,
                    });
                } else {
                    text.push_str(&manifest::format_line(algo, hex, display, tag));
                    text.push('\n');
                }
            }
            Err(e) => {
                failed += 1;
                eprintln!("filehash: {display}: {e}");

                if json {
                    json_items.push(HashedJson {
                        path: display,
                        hash: None,
                        size: None,
                        error: Some(e),
                    });
                }
            }
        }
    }

    let ok = results.iter().filter(|r| r.is_ok()).count();

    if json {
        #[derive(Serialize)]
        struct Doc<'a> {
            version: u32,
            algorithm: Algorithm,
            files: Vec<HashedJson<'a>>,
            hashed: usize,
            errors: usize,
            skipped_symlinks: usize,
        }
        let doc = Doc {
            version: 1,
            algorithm: algo,
            files: json_items,
            hashed: ok,
            errors: failed,
            skipped_symlinks: walked.skipped_symlinks,
        };
        text = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
    }

    let written = match output {
        Some(p) => write_atomic(p, text.as_bytes())
            .map_err(|e| format!("cannot write {}: {e}", p.display())),
        None => {
            let mut out = io::stdout().lock();
            match out.write_all(text.as_bytes()) {
                Err(e) if e.kind() != io::ErrorKind::BrokenPipe => {
                    Err(format!("cannot write output: {e}"))
                }
                _ => Ok(()),
            }
        }
    };

    if let Err(e) = written {
        eprintln!("filehash: {e}");
        return exit::FAILURE;
    }

    if !quiet && (output.is_some() || walked.skipped_symlinks > 0 || failed > 0) {
        eprintln!(
            "filehash: {ok} file(s) hashed, {failed} error(s), {} symlink(s) skipped",
            walked.skipped_symlinks
        );
    }

    if failed > 0 { exit::FAILURE } else { exit::OK }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Check {
    Ok,

    Failed,
    Missing,
    Error,
    Extra,
}

#[derive(Serialize)]
struct CheckJson {
    path: String,
    status: Check,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn cmd_verify(
    manifest_path: &Path,
    explicit: Option<Algorithm>,
    root: Option<&Path>,
    report_extra: bool,
    jobs: usize,
    mmap: bool,
    json: bool,
    quiet: bool,
) -> u8 {
    let text = if manifest_path.as_os_str() == "-" {
        let mut s = String::new();
        if let Err(e) = io::stdin().take(256 * 1024 * 1024).read_to_string(&mut s) {
            eprintln!("filehash: cannot read stdin: {e}");
            return exit::USAGE;
        }

        s
    } else {
        match std::fs::read_to_string(manifest_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("filehash: cannot read {}: {e}", manifest_path.display());
                return exit::USAGE;
            }
        }
    };

    let entries = match manifest::parse(&text) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("filehash: {}: {e}", manifest_path.display());
            return exit::USAGE;
        }
    };

    let name = manifest_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let default_algo = match manifest::infer_algorithm(explicit, &name, &entries) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("filehash: {}: {e}", manifest_path.display());
            return exit::USAGE;
        }
    };

    let base = root.map(Path::to_path_buf).unwrap_or_default();
    let resolved: Vec<PathBuf> = entries.iter().map(|e| base.join(&e.path)).collect();

    // Hash grouped by algorithm (manifests may mix BSD tags); usually a single group.
    let mut results: Vec<Option<pool::HashResult>> = vec![None; entries.len()];

    for algo in Algorithm::ALL {
        let idx: Vec<usize> = (0..entries.len())
            .filter(|&i| entries[i].algorithm.unwrap_or(default_algo) == algo)
            .collect();

        if idx.is_empty() {
            continue;
        }

        let paths: Vec<&Path> = idx.iter().map(|&i| resolved[i].as_path()).collect();
        for (k, r) in pool::hash_all(&paths, algo, jobs, pool::DEFAULT_BUFFER, mmap, &|_, _| {})
            .into_iter()
            .enumerate()
        {
            results[idx[k]] = Some(r);
        }
    }

    let mut checks: Vec<CheckJson> = Vec::with_capacity(entries.len());

    for ((entry, path), r) in entries.iter().zip(&resolved).zip(results) {
        let (status, detail) = match r {
            Some(Ok((hex, _))) if hex == entry.hex => (Check::Ok, None),
            Some(Ok((hex, _))) => (
                Check::Failed,
                Some(format!("expected {}, got {hex}", entry.hex)),
            ),
            Some(Err(_)) if !path.exists() => (Check::Missing, None),
            Some(Err(e)) => (Check::Error, Some(e)),
            None => (Check::Error, Some("internal error".into())),
        };

        checks.push(CheckJson {
            path: entry.path.clone(),
            status,
            detail,
        });
    }

    if report_extra {
        let walk_root = if base.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            base.clone()
        };

        let listed: std::collections::HashSet<String> = resolved
            .iter()
            .filter_map(|p| walk::display_path(&normalize(p)).ok())
            .collect();
        let manifest_norm = walk::display_path(&normalize(manifest_path)).ok();

        for f in walk::collect(&[walk_root], false, true).files {
            let norm = walk::display_path(&normalize(&f.path)).unwrap_or_default();
            if !listed.contains(&norm) && Some(&norm) != manifest_norm.as_ref() {
                checks.push(CheckJson {
                    path: norm,
                    status: Check::Extra,
                    detail: None,
                });
            }
        }
    }

    let count = |s| checks.iter().filter(|c| c.status == s).count();
    let (ok, failed, missing, errors, extra) = (
        count(Check::Ok),
        count(Check::Failed),
        count(Check::Missing),
        count(Check::Error),
        count(Check::Extra),
    );

    if json {
        #[derive(Serialize)]
        struct Doc<'a> {
            version: u32,
            results: &'a [CheckJson],
            ok: usize,
            failed: usize,
            missing: usize,
            errors: usize,
            extra: usize,
        }
        let doc = Doc {
            version: 1,
            results: &checks,
            ok,
            failed,
            missing,
            errors,
            extra,
        };
        let _ = writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::to_string_pretty(&doc).unwrap_or_default()
        );
    } else {
        let mut out = io::stdout().lock();

        for c in &checks {
            if quiet && c.status == Check::Ok {
                continue;
            }

            let label = match c.status {
                Check::Ok => "OK",
                Check::Failed => "FAILED",
                Check::Missing => "MISSING",
                Check::Error => "ERROR",
                Check::Extra => "EXTRA",
            };

            let _ = match &c.detail {
                Some(d) => writeln!(out, "{label:<8} {}: {d}", c.path),
                None => writeln!(out, "{label:<8} {}", c.path),
            };
        }

        let _ = writeln!(
            out,
            "--- {} checked: {ok} ok, {failed} failed, {missing} missing, {errors} error(s){}",
            entries.len(),
            if report_extra {
                format!(", {extra} extra")
            } else {
                String::new()
            }
        );
    }

    if failed + missing + errors + extra > 0 {
        exit::FAILURE
    } else {
        exit::OK
    }
}

/// Lexically remove `.` components so `./a` and `a` compare equal.
fn normalize(p: &Path) -> PathBuf {
    p.components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}
