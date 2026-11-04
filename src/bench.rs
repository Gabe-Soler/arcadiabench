//! The seeded batch runner: one coordinator on one scenario over many seeds
//! ([`bench`](fn@bench), `arcadiabench bench`), or across presets and variants
//! ([`suite`], `arcadiabench suite`). Runs go in parallel, each with a fresh
//! coordinator.
//!
//! What a batch writes never depends on how many runs go at once: every run
//! is independent, and runs are written and aggregated in seed order. A
//! batch directory holds `metrics.jsonl` (one [`RunRecord`] per line, in seed
//! order), `scorecard.json`, and with `save_logs` every run's event log as
//! `logs/<run_id>.jsonl.gz`. A suite writes one such directory per scenario,
//! `<preset>-<variant>/`, and `suite.json` with every scorecard and the
//! coordinator's resilience.

use crate::coordinator::{BenchError, CoordinatorLink, CoordinatorProcess};
use crate::log::EventLog;
use crate::scenario;
use crate::score::{Report, RunRecord, ScoreError, Scorecard};
use crate::session::{Outcome, run_session_outcome};
use crate::sim::params::{Params, Preset, Variant};
use crate::software_info;
use flate2::write::GzEncoder;
use flate2::{Compression, GzBuilder};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Seeds a `--seeds` list may name at most.
pub const MAX_SEEDS: usize = 100_000;

/// Parse a seed list such as `1-50` or `1,4,10-20` into ascending seeds.
pub fn parse_seeds(spec: &str) -> Result<Vec<u64>, String> {
    let bad =
        || format!("bad seed list '{spec}' (expected seeds and ranges such as 1-50 or 1,4,10-20)");
    let mut seeds = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (a, b) = match part.split_once('-') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (part, part),
        };
        let (a, b): (u64, u64) = (a.parse().map_err(|_| bad())?, b.parse().map_err(|_| bad())?);
        if a > b {
            return Err(bad());
        }
        if (b - a).saturating_add(1) > (MAX_SEEDS - seeds.len()) as u64 {
            return Err(format!(
                "seed list '{spec}' names more than {MAX_SEEDS} seeds"
            ));
        }
        seeds.extend(a..=b);
    }
    seeds.sort_unstable();
    if let Some(w) = seeds.windows(2).find(|w| w[0] == w[1]) {
        return Err(format!("seed {} is listed twice in '{spec}'", w[0]));
    }
    Ok(seeds)
}

/// Opens a link to a fresh coordinator for one run.
pub type Connect<'a> = dyn Fn() -> Result<Box<dyn CoordinatorLink>, BenchError> + Sync + 'a;

/// A coordinator command line: each run spawns it afresh, with a per-decision
/// timeout.
pub fn command(
    argv: Vec<String>,
    timeout: Duration,
) -> impl Fn() -> Result<Box<dyn CoordinatorLink>, BenchError> + Sync {
    move || {
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| BenchError::Protocol("empty coordinator command".into()))?;
        Ok(Box::new(CoordinatorProcess::spawn(program, args, timeout)?)
            as Box<dyn CoordinatorLink>)
    }
}

/// Score a finished session.
pub fn record(params: &Params, seed: u64, outcome: Outcome) -> RunRecord {
    let g = scenario::generate(seed, params);
    let report = outcome.report;
    RunRecord::new(
        &g.scenario,
        &g.mission.pad,
        seed,
        software_info(),
        report.coordinator,
        report.end_reason,
        outcome.error.map(|e| e.to_string()),
        report.metrics,
    )
}

/// Run one seed and score it. Errs only if the session never started.
pub fn run_one<W: Write>(
    link: &mut dyn CoordinatorLink,
    seed: u64,
    params: &Params,
    log: &mut EventLog<W>,
) -> Result<RunRecord, BenchError> {
    let outcome = run_session_outcome(link, seed, params, log)?;
    Ok(record(params, seed, outcome))
}

/// Why a batch failed.
#[derive(Debug)]
pub enum Error {
    /// A run never started: the coordinator could not be launched, did not
    /// welcome, or speaks another protocol version.
    Start {
        seed: u64,
        error: BenchError,
    },
    Io(PathBuf, io::Error),
    Score(ScoreError),
    /// The progress callback asked to stop before every run was done.
    Cancelled,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Start { seed, error } => write!(f, "seed {seed}: {error}"),
            Error::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Error::Score(e) => write!(f, "{e}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {}

/// How a batch runs and where it writes.
#[derive(Debug, Clone)]
pub struct Options {
    /// Runs at once (at least 1).
    pub jobs: usize,
    /// Where to write results; nothing is written without it.
    pub out: Option<PathBuf>,
    /// Also write every run's event log, gzipped (needs `out`).
    pub save_logs: bool,
}

/// One scenario's runs and their scorecard.
#[derive(Debug, Clone)]
pub struct CellResult {
    pub preset: Preset,
    pub variant: Variant,
    /// In seed order.
    pub runs: Vec<RunRecord>,
    pub scorecard: Scorecard,
}

/// A gzip stream whose flushes are deferred to [`Compressed::finish`]: the
/// event log flushes after every record, and a deflate sync point per record
/// would bloat the file. The header carries no time or name, so the bytes
/// depend only on the log.
struct Compressed(GzEncoder<BufWriter<File>>);

impl Compressed {
    fn create(path: &Path) -> io::Result<Self> {
        let file = BufWriter::new(File::create(path)?);
        Ok(Self(
            GzBuilder::new()
                .mtime(0)
                .operating_system(255)
                .write(file, Compression::default()),
        ))
    }

    fn finish(self) -> io::Result<()> {
        self.0.finish()?.flush()
    }
}

impl Write for Compressed {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// An event log file: gzipped when its name ends in `.gz`, plain JSONL
/// otherwise. Call [`LogFile::finish`] when the run is over.
pub struct LogFile(Sink);

enum Sink {
    Plain(BufWriter<File>),
    Gzip(Compressed),
}

impl LogFile {
    pub fn create(path: &Path) -> io::Result<Self> {
        let gz = path.extension().is_some_and(|e| e == "gz");
        Ok(Self(if gz {
            Sink::Gzip(Compressed::create(path)?)
        } else {
            Sink::Plain(BufWriter::new(File::create(path)?))
        }))
    }

    /// Write out everything still buffered.
    pub fn finish(self) -> io::Result<()> {
        match self.0 {
            Sink::Plain(mut w) => w.flush(),
            Sink::Gzip(z) => z.finish(),
        }
    }
}

impl Write for LogFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &mut self.0 {
            Sink::Plain(w) => w.write(buf),
            Sink::Gzip(z) => z.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.0 {
            Sink::Plain(w) => w.flush(),
            Sink::Gzip(z) => z.flush(),
        }
    }
}

struct Job {
    params: Params,
    seed: u64,
    log: Option<PathBuf>,
}

/// Run one job; the outer error is fatal for the batch.
fn run_job(connect: &Connect, job: &Job) -> Result<RunRecord, Error> {
    let start = |error| Error::Start {
        seed: job.seed,
        error,
    };
    let mut link = connect().map_err(start)?;
    match &job.log {
        None => {
            let mut log = EventLog::new(io::sink());
            run_one(link.as_mut(), job.seed, &job.params, &mut log).map_err(start)
        }
        Some(path) => {
            let io_err = |e| Error::Io(path.clone(), e);
            let mut log = EventLog::new(LogFile::create(path).map_err(io_err)?);
            let result = run_one(link.as_mut(), job.seed, &job.params, &mut log);
            if result.is_err() {
                let _ = fs::remove_file(path);
            } else {
                log.into_inner().finish().map_err(io_err)?;
            }
            result.map_err(start)
        }
    }
}

/// Run every job on `threads` threads; the records come back in job order.
/// After a fatal error no new job starts, and the first fatal error in job
/// order is returned. `progress(done, total)` hears about every finished
/// run; when it returns false, no new job starts and the batch is cancelled.
fn run_jobs(
    connect: &Connect,
    jobs: &[Job],
    threads: usize,
    progress: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Result<Vec<RunRecord>, Error> {
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let results: Vec<Mutex<Option<Result<RunRecord, Error>>>> =
        jobs.iter().map(|_| Mutex::new(None)).collect();
    thread::scope(|s| {
        for _ in 0..threads.clamp(1, jobs.len().max(1)) {
            s.spawn(|| {
                while !stop.load(Ordering::SeqCst) {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(job) = jobs.get(i) else { break };
                    let result = run_job(connect, job);
                    if result.is_err() {
                        stop.store(true, Ordering::SeqCst);
                    }
                    *results[i].lock().expect("lock") = Some(result);
                    if !progress(done.fetch_add(1, Ordering::SeqCst) + 1, jobs.len()) {
                        stop.store(true, Ordering::SeqCst);
                    }
                }
            });
        }
    });
    let mut records = Vec::with_capacity(jobs.len());
    for slot in results {
        match slot.into_inner().expect("lock") {
            Some(Ok(r)) => records.push(r),
            Some(Err(e)) => return Err(e),
            // Jobs are taken in order, so an untaken one follows every
            // finished one: the batch stopped early without a fatal error.
            None => return Err(Error::Cancelled),
        }
    }
    Ok(records)
}

fn create_dir(dir: &Path) -> Result<(), Error> {
    fs::create_dir_all(dir).map_err(|e| Error::Io(dir.into(), e))
}

fn write_file(path: &Path, text: &str) -> Result<(), Error> {
    fs::write(path, text).map_err(|e| Error::Io(path.into(), e))
}

/// Write a scenario's `metrics.jsonl` and `scorecard.json` into `dir`.
pub fn write_cell(dir: &Path, runs: &[RunRecord], scorecard: &Scorecard) -> Result<(), Error> {
    create_dir(dir)?;
    let mut lines = String::new();
    for r in runs {
        lines.push_str(&serde_json::to_string(r).expect("records serialize"));
        lines.push('\n');
    }
    write_file(&dir.join("metrics.jsonl"), &lines)?;
    write_file(&dir.join("scorecard.json"), &to_pretty(scorecard))
}

/// Pretty JSON with a final newline.
pub fn to_pretty<T: serde::Serialize>(value: &T) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("scorecards serialize");
    text.push('\n');
    text
}

/// The directory a suite writes a scenario's results to.
pub fn cell_dir(out: &Path, preset: Preset, variant: Variant) -> PathBuf {
    out.join(format!("{preset}-{variant}"))
}

/// Run every seed of every scenario, each scenario's results going to
/// `dir(preset, variant)` when writing.
fn run_cells(
    connect: &Connect,
    cells: &[(Preset, Variant)],
    seeds: &[u64],
    opts: &Options,
    dir: impl Fn(&Path, Preset, Variant) -> PathBuf,
    progress: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Result<Vec<CellResult>, Error> {
    let mut jobs = Vec::new();
    for &(preset, variant) in cells {
        let params = Params::scenario(preset, variant);
        let logs = match &opts.out {
            Some(out) if opts.save_logs => {
                let logs = dir(out, preset, variant).join("logs");
                create_dir(&logs)?;
                Some(logs)
            }
            _ => None,
        };
        for &seed in seeds {
            let id = scenario::scenario_id(preset, variant);
            jobs.push(Job {
                params: params.clone(),
                seed,
                log: logs
                    .as_ref()
                    .map(|d| d.join(format!("{id}-{seed}.jsonl.gz"))),
            });
        }
    }
    let records = run_jobs(connect, &jobs, opts.jobs, progress)?;
    let mut out = Vec::new();
    for (k, &(preset, variant)) in cells.iter().enumerate() {
        let runs = records[k * seeds.len()..(k + 1) * seeds.len()].to_vec();
        let scorecard = Scorecard::from_runs(&runs).map_err(Error::Score)?;
        if let Some(root) = &opts.out {
            write_cell(&dir(root, preset, variant), &runs, &scorecard)?;
        }
        out.push(CellResult {
            preset,
            variant,
            runs,
            scorecard,
        });
    }
    Ok(out)
}

/// Run a coordinator on one scenario over `seeds`, writing into `opts.out`.
pub fn bench(
    connect: &Connect,
    preset: Preset,
    variant: Variant,
    seeds: &[u64],
    opts: &Options,
    progress: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Result<CellResult, Error> {
    let mut cells = run_cells(
        connect,
        &[(preset, variant)],
        seeds,
        opts,
        |out, _, _| out.to_path_buf(),
        progress,
    )?;
    Ok(cells.remove(0))
}

/// A suite's scenarios and its report.
#[derive(Debug, Clone)]
pub struct SuiteResult {
    pub cells: Vec<CellResult>,
    pub report: Report,
}

/// Run a coordinator on every preset × variant over `seeds`, writing each
/// scenario into `opts.out/<preset>-<variant>/` and the report into
/// `opts.out/suite.json`.
pub fn suite(
    connect: &Connect,
    presets: &[Preset],
    variants: &[Variant],
    seeds: &[u64],
    opts: &Options,
    progress: &(dyn Fn(usize, usize) -> bool + Sync),
) -> Result<SuiteResult, Error> {
    let cells: Vec<(Preset, Variant)> = presets
        .iter()
        .flat_map(|&p| variants.iter().map(move |&v| (p, v)))
        .collect();
    let cells = run_cells(connect, &cells, seeds, opts, cell_dir, progress)?;
    let runs: Vec<RunRecord> = cells.iter().flat_map(|c| c.runs.iter().cloned()).collect();
    let report = Report::from_runs(&runs).map_err(Error::Score)?;
    if let Some(out) = &opts.out {
        write_file(&out.join("suite.json"), &to_pretty(&report))?;
    }
    Ok(SuiteResult { cells, report })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_lists_parse() {
        assert_eq!(parse_seeds("1-5").unwrap(), [1, 2, 3, 4, 5]);
        assert_eq!(parse_seeds("7, 1-3,10").unwrap(), [1, 2, 3, 7, 10]);
        assert_eq!(parse_seeds("0").unwrap(), [0]);
        for bad in ["", "5-1", "a", "1-", "1,,2", "-3"] {
            assert!(parse_seeds(bad).is_err(), "{bad}");
        }
        assert!(parse_seeds("1-3,2").unwrap_err().contains("twice"));
        assert!(parse_seeds("0-18446744073709551615").is_err());
        assert_eq!(parse_seeds("1-100000").unwrap().len(), MAX_SEEDS);
        assert!(parse_seeds("1-100000,100001").is_err());
    }
}
