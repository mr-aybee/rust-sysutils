use clap::Parser;
use disklens::glob::Pattern;
use disklens::render::{self, View};
use disklens::scan::{self, Options};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const EXIT_OK: u8 = 0;
/// Scan completed but some entries could not be read (like `du`).
const EXIT_PARTIAL: u8 = 1;
const EXIT_USAGE: u8 = 2;

#[derive(Debug, Parser)]
#[command(
    name = "disklens",
    version,
    about = "Fast parallel disk usage analysis"
)]
struct Cli {
    /// Directory (or file) to analyse.
    #[arg(default_value = ".")]
    path: PathBuf,
    /// Directory levels to display (the scan itself is always complete).
    #[arg(short, long, default_value_t = 1)]
    depth: usize,
    /// Maximum subdirectories shown per directory; the rest are summarised.
    #[arg(short, long, default_value_t = 20)]
    limit: usize,
    /// Number of largest files to list (0 to disable).
    #[arg(short, long, default_value_t = 10, value_parser = clap::value_parser!(u32).range(0..=10_000))]
    top: u32,
    /// Rank by apparent (logical) size instead of space allocated on disk.
    #[arg(short, long)]
    apparent: bool,
    /// Exclude entries matching a glob (repeatable). No '/': matches names; with '/': matches paths relative to the root. Supports *, ?, **.
    #[arg(short, long, value_name = "GLOB")]
    exclude: Vec<String>,
    /// Do not descend into directories on other filesystems (Unix).
    #[arg(short = 'x', long)]
    one_file_system: bool,
    /// Worker threads (default: available CPUs).
    #[arg(short = 'j', long, value_parser = clap::value_parser!(u16).range(1..=256))]
    jobs: Option<u16>,
    /// Print sizes in bytes.
    #[arg(short, long)]
    bytes: bool,
    /// Emit JSON (tree limited to --depth).
    #[arg(long)]
    json: bool,
    /// List individual read errors (first 100).
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(if e.use_stderr() { EXIT_USAGE } else { EXIT_OK });
        }
    };

    let exclude = match cli
        .exclude
        .iter()
        .map(|p| Pattern::new(p))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(e) => e,
        Err(e) => {
            eprintln!("disklens: {e}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    if let Some(j) = cli.jobs
        && let Err(e) = rayon::ThreadPoolBuilder::new()
            .num_threads(usize::from(j))
            .build_global()
    {
        eprintln!("disklens: cannot configure thread pool: {e}");
        return ExitCode::from(EXIT_USAGE);
    }

    let opts = Options {
        one_file_system: cli.one_file_system,
        exclude,
        top_files: cli.top as usize,
        apparent: cli.apparent,
    };

    let mut result = match scan::scan(&cli.path, &opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("disklens: {e}");
            return ExitCode::from(EXIT_PARTIAL);
        }
    };

    let status = if result.errors > 0 {
        EXIT_PARTIAL
    } else {
        EXIT_OK
    };

    if cli.verbose || (!cli.json && result.errors > 0) {
        let shown = if cli.verbose {
            result.error_samples.len()
        } else {
            result.error_samples.len().min(5)
        };

        for e in &result.error_samples[..shown] {
            eprintln!("disklens: {}: {}", e.path, e.error);
        }

        if (result.errors as usize) > shown {
            eprintln!(
                "disklens: … {} more error(s){}",
                result.errors as usize - shown,
                if cli.verbose { "" } else { " (use -v)" }
            );
        }
    }

    scan::prune(&mut result.root, cli.depth);

    let text = if cli.json {
        #[derive(serde::Serialize)]
        struct Doc<'a> {
            version: u32,
            ranked_by: &'static str,
            #[serde(flatten)]
            result: &'a scan::ScanResult,
        }
        let doc = Doc {
            version: 1,
            ranked_by: if cli.apparent { "logical" } else { "allocated" },
            result: &result,
        };
        serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
    } else {
        render::render(
            &result,
            &View {
                depth: cli.depth,
                limit: cli.limit.max(1),
                apparent: cli.apparent || !result.allocated_supported,
                raw: cli.bytes,
            },
        )
    };

    let mut out = std::io::stdout().lock();
    if let Err(e) = out.write_all(text.as_bytes())
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        eprintln!("disklens: cannot write output: {e}");
        return ExitCode::from(EXIT_PARTIAL);
    }

    ExitCode::from(status)
}
