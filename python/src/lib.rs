//! The native half of the `arcadiabench` Python package: ArcadiaBench's
//! runs, batches, scoring and scenarios. The Python half
//! (`arcadiabench/__init__.py`) wraps these functions in the public API.
//!
//! Messages cross the boundary as JSON: protocol messages go to Python
//! coordinators as dicts, and what they return is checked against the
//! protocol's schemas before the simulator sees it.

use arcadia_protocol::{PROTOCOL_VERSION, SimToCoordinator};
use arcadiabench::bench::{self, LogFile, Options};
use arcadiabench::coordinator::{CoordinatorProcess, InProcessJson, JsonAgent};
use arcadiabench::log::EventLog;
use arcadiabench::score::{Report, RunRecord, SCHEMA, Scorecard, logs};
use arcadiabench::session::{self, Outcome, run_session_outcome};
use arcadiabench::sim::params::{Params, Preset, Variant};
use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyOSError, PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

create_exception!(
    arcadiabench,
    CoordinatorError,
    PyRuntimeError,
    "A coordinator could not start a run, or failed, timed out or broke the protocol during one. \
     When the run got under way, `result` holds its run record."
);

/// A Rust value as Python objects, through JSON.
fn to_py<'py>(py: Python<'py>, value: &impl Serialize) -> PyResult<Bound<'py, PyAny>> {
    let text = serde_json::to_string(value).map_err(|e| PyValueError::new_err(e.to_string()))?;
    py.import("json")?.call_method1("loads", (text,))
}

/// Python objects as a JSON value. NaN and infinity are refused.
fn from_py(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    let py = value.py();
    let kwargs = PyDict::new(py);
    kwargs.set_item("allow_nan", false)?;
    let text: String = py
        .import("json")?
        .call_method("dumps", (value,), Some(&kwargs))?
        .extract()?;
    serde_json::from_str(&text).map_err(|e| PyValueError::new_err(e.to_string()))
}

fn scenario_params(preset: &str, variant: &str) -> PyResult<(Preset, Variant, Params)> {
    let p: Preset = preset.parse().map_err(PyValueError::new_err)?;
    let v: Variant = variant.parse().map_err(PyValueError::new_err)?;
    Ok((p, v, Params::scenario(p, v)))
}

fn timeout(seconds: f64) -> PyResult<Duration> {
    if seconds > 0.0 && seconds.is_finite() {
        Ok(Duration::from_secs_f64(seconds))
    } else {
        Err(PyValueError::new_err(format!(
            "decision_timeout must be positive, not {seconds}"
        )))
    }
}

fn jobs(n: usize) -> usize {
    if n == 0 {
        thread::available_parallelism().map_or(1, |n| n.get())
    } else {
        n
    }
}

fn os_error(path: &Path, e: io::Error) -> PyErr {
    PyOSError::new_err(format!("{}: {e}", path.display()))
}

/// A Python coordinator: an object with `welcome`, `decide` and, optionally,
/// `end`. A Python exception in any of them is kept, to be raised again once
/// the run is over and its log closed.
struct PyAgent {
    obj: Py<PyAny>,
    error: Option<PyErr>,
}

impl PyAgent {
    fn call(&mut self, method: &str, message: Value) -> Result<Value, String> {
        let result = Python::attach(|py| -> PyResult<Value> {
            let obj = self.obj.bind(py);
            if method == "end" && !obj.hasattr("end")? {
                return Ok(Value::Null);
            }
            let reply = obj.call_method1(method, (to_py(py, &message)?,))?;
            if method != "end" && !reply.is_instance_of::<PyDict>() {
                return Err(PyTypeError::new_err(format!(
                    "{method}() must return a dict, not {}",
                    reply.get_type().name()?
                )));
            }
            from_py(&reply)
        });
        result.map_err(|e| {
            let text = e.to_string();
            self.error = Some(e);
            text
        })
    }
}

impl JsonAgent for PyAgent {
    fn welcome(&mut self, hello: Value) -> Result<Value, String> {
        self.call("welcome", hello)
    }

    fn decide(&mut self, observation: Value) -> Result<Value, String> {
        self.call("decide", observation)
    }

    fn end(&mut self, end: Value) -> Result<(), String> {
        self.call("end", end).map(|_| ())
    }
}

/// Whether an error must stop everything at once (Ctrl-C, `sys.exit`).
fn is_fatal(py: Python<'_>, e: &PyErr) -> bool {
    !e.is_instance_of::<PyException>(py)
}

/// A `CoordinatorError` with `message`, caused by the coordinator's own
/// exception if it raised one, and carrying the run's result if there is one.
fn coordinator_error(
    py: Python<'_>,
    message: String,
    cause: Option<PyErr>,
    result: Option<Bound<'_, PyAny>>,
) -> PyResult<PyErr> {
    let err = CoordinatorError::new_err(message);
    err.set_cause(py, cause);
    let result = result.unwrap_or_else(|| py.None().into_bound(py));
    err.value(py).setattr("result", result)?;
    Ok(err)
}

/// A run's record as a dict, with the session report's extras: when the run
/// ended, the share of the pad in spec, and the lost-link returns.
fn run_dict<'py>(
    py: Python<'py>,
    record: &RunRecord,
    end_t: f64,
    progress: f64,
    lost_link_returns: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let d = to_py(py, record)?;
    d.set_item("end_t", end_t)?;
    d.set_item("progress", progress)?;
    d.set_item("lost_link_returns", lost_link_returns)?;
    Ok(d)
}

/// Run `work` with the GIL released, watching for Ctrl-C on this thread
/// meanwhile. The flag it gets turns true when Ctrl-C comes; the interrupt
/// is returned beside the work's result.
fn interruptible<T: Send>(
    py: Python<'_>,
    work: impl FnOnce(&AtomicBool) -> T + Send,
) -> (T, Option<PyErr>) {
    let cancelled = AtomicBool::new(false);
    let mut interrupt = None;
    let result = py.detach(|| {
        thread::scope(|s| {
            let handle = s.spawn(|| work(&cancelled));
            while !handle.is_finished() {
                thread::sleep(Duration::from_millis(20));
                if interrupt.is_none()
                    && let Err(e) = Python::attach(|py| py.check_signals())
                {
                    interrupt = Some(e);
                    cancelled.store(true, Ordering::SeqCst);
                }
            }
            handle.join().expect("the work does not panic")
        })
    });
    (result, interrupt)
}

/// Where a run's event log goes: nowhere, or a file.
enum Sink {
    Null,
    File(LogFile),
}

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Sink::Null => Ok(buf.len()),
            Sink::File(f) => f.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Sink::Null => Ok(()),
            Sink::File(f) => f.flush(),
        }
    }
}

/// Run a session into an optional log file (gzipped if its name ends in
/// `.gz`), finishing the file afterwards.
fn with_log<T>(
    log_path: Option<&Path>,
    session: impl FnOnce(&mut EventLog<Sink>) -> T,
) -> PyResult<T> {
    let sink = match log_path {
        None => Sink::Null,
        Some(path) => Sink::File(LogFile::create(path).map_err(|e| os_error(path, e))?),
    };
    let mut log = EventLog::new(sink);
    let result = session(&mut log);
    if let (Sink::File(file), Some(path)) = (log.into_inner(), log_path) {
        file.finish().map_err(|e| os_error(path, e))?;
    }
    Ok(result)
}

/// Turn a finished session into its run record, or the error to raise.
fn conclude(
    py: Python<'_>,
    params: &Params,
    seed: u64,
    outcome: Result<Outcome, arcadiabench::coordinator::BenchError>,
    cause: Option<PyErr>,
) -> PyResult<Py<PyAny>> {
    match outcome {
        Err(e) => Err(coordinator_error(py, e.to_string(), cause, None)?),
        Ok(outcome) => {
            let error = outcome.error.as_ref().map(|e| e.to_string());
            let r = &outcome.report;
            let (end_t, progress, returns) = (r.end_t, r.progress, r.lost_link_returns);
            let record = bench::record(params, seed, outcome);
            let result = run_dict(py, &record, end_t, progress, returns)?;
            match error {
                Some(message) => Err(coordinator_error(py, message, cause, Some(result))?),
                None => Ok(result.unbind()),
            }
        }
    }
}

/// Run one seed with a subprocess coordinator. Returns the run record and
/// the session report's extras.
#[pyfunction]
#[pyo3(signature = (argv, seed, preset, variant, log_path, decision_timeout))]
fn run_process(
    py: Python<'_>,
    argv: Vec<String>,
    seed: u64,
    preset: &str,
    variant: &str,
    log_path: Option<PathBuf>,
    decision_timeout: f64,
) -> PyResult<Py<PyAny>> {
    let (_, _, params) = scenario_params(preset, variant)?;
    let limit = timeout(decision_timeout)?;
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| PyValueError::new_err("empty coordinator command"))?;
    let (outcome, interrupt) = interruptible(py, |_| {
        with_log(log_path.as_deref(), |log| {
            let mut link = CoordinatorProcess::spawn(program, args, limit)?;
            run_session_outcome(&mut link, seed, &params, log)
        })
    });
    if let Some(e) = interrupt {
        return Err(e);
    }
    conclude(py, &params, seed, outcome?, None)
}

/// Run one seed with an in-process Python coordinator. Returns the run
/// record and the session report's extras.
#[pyfunction]
#[pyo3(signature = (agent, seed, preset, variant, log_path, decision_timeout))]
fn run_agent(
    py: Python<'_>,
    agent: Py<PyAny>,
    seed: u64,
    preset: &str,
    variant: &str,
    log_path: Option<PathBuf>,
    decision_timeout: f64,
) -> PyResult<Py<PyAny>> {
    let (_, _, params) = scenario_params(preset, variant)?;
    let limit = timeout(decision_timeout)?;
    let mut link = InProcessJson::new(
        PyAgent {
            obj: agent,
            error: None,
        },
        Some(limit),
    )
    .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
    let outcome = with_log(log_path.as_deref(), |log| {
        run_session_outcome(&mut link, seed, &params, log)
    })?;
    let cause = link.agent.error.take();
    if let Some(e) = &cause
        && is_fatal(py, e)
    {
        return Err(cause.expect("checked"));
    }
    conclude(py, &params, seed, outcome, cause)
}

/// A batch's result as a dict: its scorecard and its run records.
fn cell_dict(py: Python<'_>, scorecard: &Scorecard, runs: &[RunRecord]) -> PyResult<Py<PyAny>> {
    let d = PyDict::new(py);
    d.set_item("scorecard", to_py(py, scorecard)?)?;
    d.set_item("runs", to_py(py, &runs)?)?;
    Ok(d.into_any().unbind())
}

fn batch_error(py: Python<'_>, e: bench::Error) -> PyResult<PyErr> {
    Ok(match e {
        bench::Error::Start { .. } => coordinator_error(py, e.to_string(), None, None)?,
        bench::Error::Io(..) => PyOSError::new_err(e.to_string()),
        bench::Error::Score(_) | bench::Error::Cancelled => PyRuntimeError::new_err(e.to_string()),
    })
}

/// Run a subprocess coordinator over seeds, in parallel.
#[pyfunction]
#[pyo3(signature = (argv, seeds, preset, variant, jobs, out, save_logs, decision_timeout))]
#[allow(clippy::too_many_arguments)]
fn bench_process(
    py: Python<'_>,
    argv: Vec<String>,
    seeds: Vec<u64>,
    preset: &str,
    variant: &str,
    jobs: usize,
    out: Option<PathBuf>,
    save_logs: bool,
    decision_timeout: f64,
) -> PyResult<Py<PyAny>> {
    let (p, v, _) = scenario_params(preset, variant)?;
    let connect = bench::command(argv, timeout(decision_timeout)?);
    let opts = Options {
        jobs: self::jobs(jobs),
        out,
        save_logs,
    };
    let (result, interrupt) = interruptible(py, |cancelled| {
        bench::bench(&connect, p, v, &seeds, &opts, &|_, _| {
            !cancelled.load(Ordering::SeqCst)
        })
    });
    if let Some(e) = interrupt {
        return Err(e);
    }
    let cell = result.map_err(|e| batch_error(py, e).unwrap_or_else(|err| err))?;
    cell_dict(py, &cell.scorecard, &cell.runs)
}

/// Run an in-process Python coordinator over seeds, one after another. The
/// factory makes a fresh coordinator for each run. Coordinator errors are
/// recorded in the runs, not raised.
#[pyfunction]
#[pyo3(signature = (factory, seeds, preset, variant, out, save_logs, decision_timeout))]
#[allow(clippy::too_many_arguments)]
fn bench_agent(
    py: Python<'_>,
    factory: Bound<'_, PyAny>,
    seeds: Vec<u64>,
    preset: &str,
    variant: &str,
    out: Option<PathBuf>,
    save_logs: bool,
    decision_timeout: f64,
) -> PyResult<Py<PyAny>> {
    let (p, v, params) = scenario_params(preset, variant)?;
    let limit = timeout(decision_timeout)?;
    let logs = match &out {
        Some(dir) if save_logs => {
            let logs = dir.join("logs");
            std::fs::create_dir_all(&logs).map_err(|e| os_error(&logs, e))?;
            Some(logs)
        }
        _ => None,
    };
    let id = arcadiabench::scenario::scenario_id(p, v);
    let mut runs = Vec::with_capacity(seeds.len());
    for &seed in &seeds {
        let mut link = InProcessJson::new(
            PyAgent {
                obj: factory.call0()?.unbind(),
                error: None,
            },
            Some(limit),
        )
        .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
        let path = logs
            .as_ref()
            .map(|d| d.join(format!("{id}-{seed}.jsonl.gz")));
        let outcome = with_log(path.as_deref(), |log| {
            run_session_outcome(&mut link, seed, &params, log)
        })?;
        let cause = link.agent.error.take();
        if let Some(e) = &cause
            && is_fatal(py, e)
        {
            return Err(cause.expect("checked"));
        }
        match outcome {
            Ok(outcome) => runs.push(bench::record(&params, seed, outcome)),
            Err(e) => {
                if let Some(path) = &path {
                    let _ = std::fs::remove_file(path);
                }
                return Err(coordinator_error(
                    py,
                    format!("seed {seed}: {e}"),
                    cause,
                    None,
                )?);
            }
        }
    }
    let scorecard =
        Scorecard::from_runs(&runs).map_err(|e| PyValueError::new_err(e.to_string()))?;
    if let Some(dir) = &out {
        bench::write_cell(dir, &runs, &scorecard).map_err(|e| PyOSError::new_err(e.to_string()))?;
    }
    cell_dict(py, &scorecard, &runs)
}

/// Run a subprocess coordinator on every preset × variant over seeds.
#[pyfunction]
#[pyo3(signature = (argv, seeds, presets, variants, jobs, out, save_logs, decision_timeout))]
#[allow(clippy::too_many_arguments)]
fn suite_process(
    py: Python<'_>,
    argv: Vec<String>,
    seeds: Vec<u64>,
    presets: Vec<String>,
    variants: Vec<String>,
    jobs: usize,
    out: Option<PathBuf>,
    save_logs: bool,
    decision_timeout: f64,
) -> PyResult<Py<PyAny>> {
    let presets = presets
        .iter()
        .map(|p| p.parse::<Preset>().map_err(PyValueError::new_err))
        .collect::<PyResult<Vec<_>>>()?;
    let variants = variants
        .iter()
        .map(|v| v.parse::<Variant>().map_err(PyValueError::new_err))
        .collect::<PyResult<Vec<_>>>()?;
    let connect = bench::command(argv, timeout(decision_timeout)?);
    let opts = Options {
        jobs: self::jobs(jobs),
        out,
        save_logs,
    };
    let (result, interrupt) = interruptible(py, |cancelled| {
        bench::suite(&connect, &presets, &variants, &seeds, &opts, &|_, _| {
            !cancelled.load(Ordering::SeqCst)
        })
    });
    if let Some(e) = interrupt {
        return Err(e);
    }
    let suite = result.map_err(|e| batch_error(py, e).unwrap_or_else(|err| err))?;
    let runs: Vec<RunRecord> = suite
        .cells
        .iter()
        .flat_map(|c| c.runs.iter().cloned())
        .collect();
    let d = PyDict::new(py);
    d.set_item("report", to_py(py, &suite.report)?)?;
    d.set_item("runs", to_py(py, &runs)?)?;
    Ok(d.into_any().unbind())
}

/// Scorecards and resilience from saved event logs and directories of them.
#[pyfunction]
fn score(py: Python<'_>, paths: Vec<PathBuf>) -> PyResult<Py<PyAny>> {
    let runs = py
        .detach(|| logs::read_logs(&paths))
        .map_err(|e| PyOSError::new_err(e.to_string()))?;
    if runs.is_empty() {
        return Err(PyValueError::new_err("no event logs found"));
    }
    let report = Report::from_runs(&runs).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(to_py(py, &report)?.unbind())
}

/// The `hello` message a coordinator gets for this scenario and seed.
#[pyfunction]
fn scenario(py: Python<'_>, seed: u64, preset: &str, variant: &str) -> PyResult<Py<PyAny>> {
    let (_, _, params) = scenario_params(preset, variant)?;
    let hello = SimToCoordinator::Hello(Box::new(session::hello(seed, &params)));
    Ok(to_py(py, &hello)?.unbind())
}

/// Seeds from a list such as `1-50` or `1,4,10-20`.
#[pyfunction]
fn parse_seeds(spec: &str) -> PyResult<Vec<u64>> {
    bench::parse_seeds(spec).map_err(PyValueError::new_err)
}

/// The `arcadiabench` command line, with `args` after the program name.
/// Returns the exit code.
#[pyfunction]
fn cli(py: Python<'_>, args: Vec<String>) -> u8 {
    py.detach(|| arcadiabench::cli::main(args))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(run_process, m)?)?;
    m.add_function(wrap_pyfunction!(run_agent, m)?)?;
    m.add_function(wrap_pyfunction!(bench_process, m)?)?;
    m.add_function(wrap_pyfunction!(bench_agent, m)?)?;
    m.add_function(wrap_pyfunction!(suite_process, m)?)?;
    m.add_function(wrap_pyfunction!(score, m)?)?;
    m.add_function(wrap_pyfunction!(scenario, m)?)?;
    m.add_function(wrap_pyfunction!(parse_seeds, m)?)?;
    m.add_function(wrap_pyfunction!(cli, m)?)?;
    m.add("CoordinatorError", m.py().get_type::<CoordinatorError>())?;
    m.add("PROTOCOL_VERSION", PROTOCOL_VERSION)?;
    m.add("VERSION", arcadiabench::software_info().version)?;
    m.add("PRESETS", Preset::ALL.map(Preset::name).to_vec())?;
    m.add("VARIANTS", Variant::ALL.map(Variant::name).to_vec())?;
    m.add(
        "STRESS_VARIANTS",
        Variant::STRESS.map(Variant::name).to_vec(),
    )?;
    m.add("SCORECARD_SCHEMA", SCHEMA)?;
    Ok(())
}
