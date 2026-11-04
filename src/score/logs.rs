//! Run records from saved event logs: `.jsonl`, or gzipped `.jsonl.gz` as
//! `arcadiabench bench --save-logs` writes them.
//!
//! A log is scored from three of its records: `run_start` (seed, scenario,
//! coordinator), the goal Earth sent at the start (the pad), and `run_end`
//! (reason and metrics). Nothing else is parsed.
//!
//! A bench's `metrics.jsonl` already holds run records, one per line. Named
//! as a file, it is read as it is, so runs from several benches can be
//! scored together without their logs. A directory is searched for event
//! logs only, so a bench directory never yields the same run twice.

use super::RunRecord;
use arcadia_protocol::*;
use flate2::read::GzDecoder;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;

/// Why a log could not be read.
#[derive(Debug)]
pub enum LogError {
    Io(PathBuf, io::Error),
    /// The file is not a complete event log.
    Invalid(PathBuf, String),
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            LogError::Invalid(p, why) => write!(f, "{}: {why}", p.display()),
        }
    }
}

impl std::error::Error for LogError {}

fn is_log_name(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".jsonl") || name.ends_with(".jsonl.gz")
}

fn open(path: &Path) -> Result<Box<dyn BufRead>, LogError> {
    let file = File::open(path).map_err(|e| LogError::Io(path.into(), e))?;
    let gz = path.extension().is_some_and(|e| e == "gz");
    Ok(if gz {
        Box::new(BufReader::new(GzDecoder::new(file)))
    } else {
        Box::new(BufReader::new(file))
    })
}

/// Score the run in one event log.
pub fn read_log(path: &Path) -> Result<RunRecord, LogError> {
    let lines = open(path)?.lines();
    record_from_lines(lines).map_err(|why| match why {
        Why::Io(e) => LogError::Io(path.into(), e),
        Why::Invalid(s) => LogError::Invalid(path.into(), s),
    })
}

enum Why {
    Io(io::Error),
    Invalid(String),
}

fn parse(line: &str) -> Result<LogRecord, Why> {
    serde_json::from_str(line).map_err(|e| Why::Invalid(format!("unreadable record: {e}")))
}

/// Score the run in an event log, given its lines.
fn record_from_lines(lines: impl Iterator<Item = io::Result<String>>) -> Result<RunRecord, Why> {
    let mut start = None;
    let mut pad = None;
    let mut last = None;
    for line in lines {
        let line = line.map_err(Why::Io)?;
        if line.trim().is_empty() {
            continue;
        }
        if start.is_none() {
            match parse(&line)?.entry {
                LogEntry::RunStart {
                    simulator,
                    coordinator,
                    seed,
                    scenario,
                    ..
                } => start = Some((simulator, coordinator, seed, scenario)),
                _ => return Err(Why::Invalid("not an event log: no run_start".into())),
            }
        } else if pad.is_none()
            && line.contains("\"type\":\"earth_message\"")
            && let LogEntry::Observed { event } = parse(&line)?.entry
            && let EventKind::EarthMessage { message, .. } = event.kind
            && let Uplink::Goal {
                objective: Objective::LandingPad { pad: spec },
                ..
            } = message.body
        {
            pad = Some(spec);
        }
        last = Some(line);
    }
    let (simulator, coordinator, seed, scenario) =
        start.ok_or_else(|| Why::Invalid("empty log".into()))?;
    let pad = pad.ok_or_else(|| Why::Invalid("no landing-pad goal".into()))?;
    let end = last.map(|l| parse(&l)).transpose()?;
    let Some(LogRecord {
        entry: LogEntry::RunEnd { reason, metrics },
        ..
    }) = end
    else {
        return Err(Why::Invalid("incomplete log: no run_end".into()));
    };
    let coordinator = coordinator.unwrap_or_else(|| SoftwareInfo {
        name: "unknown".into(),
        version: "0.0.0".into(),
    });
    Ok(RunRecord::new(
        &scenario,
        &pad,
        seed,
        simulator,
        coordinator,
        reason,
        None,
        metrics,
    ))
}

/// The first line of a file.
fn first_line(path: &Path) -> Result<String, LogError> {
    let mut first = String::new();
    open(path)?
        .read_line(&mut first)
        .map_err(|e| LogError::Io(path.into(), e))?;
    Ok(first)
}

/// Whether a file starts like an event log.
fn starts_like_a_log(path: &Path) -> Result<bool, LogError> {
    Ok(first_line(path)?.contains("\"kind\":\"run_start\""))
}

/// The run records of a bench's `metrics.jsonl`, one per line.
pub fn read_records(path: &Path) -> Result<Vec<RunRecord>, LogError> {
    let mut out = Vec::new();
    for line in open(path)?.lines() {
        let line = line.map_err(|e| LogError::Io(path.into(), e))?;
        if line.trim().is_empty() {
            continue;
        }
        let record = serde_json::from_str(&line)
            .map_err(|e| LogError::Invalid(path.into(), format!("not a run record: {e}")))?;
        out.push(record);
    }
    Ok(out)
}

/// The runs in one file: an event log's single run, or every record of a
/// bench's `metrics.jsonl`.
fn read_file(path: &Path) -> Result<Vec<RunRecord>, LogError> {
    if starts_like_a_log(path)? || !first_line(path)?.contains("\"run_id\"") {
        read_log(path).map(|r| vec![r])
    } else {
        read_records(path)
    }
}

/// The event logs among `paths`: files as given, and every `.jsonl` or
/// `.jsonl.gz` file under a directory that starts like an event log (so a
/// bench's `metrics.jsonl` is skipped). Sorted, without duplicates.
pub fn find_logs(paths: &[PathBuf]) -> Result<Vec<PathBuf>, LogError> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), LogError> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| LogError::Io(dir.into(), e))?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<_, _>>()
            .map_err(|e| LogError::Io(dir.into(), e))?;
        entries.sort();
        for p in entries {
            if p.is_dir() {
                walk(&p, out)?;
            } else if is_log_name(&p) && starts_like_a_log(&p)? {
                out.push(p);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            walk(p, &mut out)?;
        } else {
            out.push(p.clone());
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// One file's runs, once a reader thread has read it.
type Slot = Mutex<Option<Result<Vec<RunRecord>, LogError>>>;

/// Score every log in `paths` (see [`find_logs`]), reading them in parallel;
/// a `metrics.jsonl` named as a file gives its run records. The records
/// come back in the order of the sorted paths.
pub fn read_logs(paths: &[PathBuf]) -> Result<Vec<RunRecord>, LogError> {
    let files = find_logs(paths)?;
    let jobs = thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(files.len().max(1));
    let next = Mutex::new(0usize);
    let results: Vec<Slot> = files.iter().map(|_| Mutex::new(None)).collect();
    thread::scope(|s| {
        for _ in 0..jobs {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().expect("lock");
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some(path) = files.get(i) else { break };
                    *results[i].lock().expect("lock") = Some(read_file(path));
                }
            });
        }
    });
    let mut out = Vec::new();
    for r in results {
        out.extend(r.into_inner().expect("lock").expect("every file is read")?);
    }
    Ok(out)
}
