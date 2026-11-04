# arcadiabench

Python bindings for [ArcadiaBench](https://github.com/Gabe-Soler/arcadiabench), the open, seeded benchmark for coordinating robot crews building on Mars. Run, benchmark and score coordinators, written in Python or anything else, from Python.

The package wraps ArcadiaBench's Rust simulator (PyO3 + maturin). It is one abi3 wheel for CPython 3.9 and later. Version 1.0.0 (`arcadiabench.__version__`) is ArcadiaBench 1.0 and speaks arcadia-protocol 1.0.0 (`arcadiabench.PROTOCOL_VERSION`).

## Install

From a checkout of ArcadiaBench (Rust and [uv](https://docs.astral.sh/uv/) required):

```sh
cd python
uv venv .venv
uv pip install --python .venv maturin pytest
.venv/bin/maturin develop --uv --release    # build and install into .venv
.venv/bin/python -m pytest                   # run the tests
```

`maturin develop` without `--release` builds faster but simulates more slowly. To build a wheel instead, run `maturin build --release --out dist`, then install it with `pip install dist/*.whl`.

## Coordinators

A coordinator is one of two things:

- **A command line:** a list of strings, or one path. It runs as a subprocess speaking newline-delimited JSON on stdin and stdout, like any ArcadiaBench coordinator.
- **A Python object** with `welcome(hello)`, `decide(observation)` and optionally `end(end)`. It runs in process.

Either way, every message the coordinator sends is checked against [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol)'s JSON Schemas, and messages are plain dicts with the schemas' shapes. A reply may leave out its `type`, and a welcome its `protocol_version`; both are filled in.

`arcadiabench.Coordinator` is a base class whose defaults welcome every run and never act:

```python
import arcadiabench

class Charger(arcadiabench.Coordinator):
    """Sends every idle robot below half charge to the first station."""

    name = "charger"
    version = "1.0.0"

    def welcome(self, hello):
        scenario = hello["scenario"]
        self.station = scenario["stations"][0]["station_id"]
        self.capacity = {r["robot_id"]: r["battery_capacity_wh"] for r in scenario["robots"]}
        self.n = 0
        return super().welcome(hello)

    def decide(self, observation):
        commands = []
        for r in observation["robots"]:
            low = r["battery_wh"] < 0.5 * self.capacity[r["robot_id"]]
            if low and r["status"] == "idle" and r.get("current_command") is None:
                self.n += 1
                commands.append({"command_id": f"c{self.n}", "robot_id": r["robot_id"],
                                 "type": "charge", "source": self.station})
        return self.decision(observation, commands=commands)

if __name__ == "__main__":
    arcadiabench.serve(Charger())
```

`arcadiabench.serve(coordinator)` serves one run over stdin and stdout, so the same file runs under the command line:

```sh
arcadiabench run --seed 1 -- python charger.py
```

While it serves, anything the coordinator prints goes to stderr, where it cannot corrupt the protocol.

## API

| Function | Returns |
|---|---|
| `run(coordinator, seed=0, preset="small", variant="nominal", log_path=None, decision_timeout=10.0)` | The run record: run id, scenario, seed, coordinator, end reason, `reference_sols`, `speed`, `score`, and `metrics` (`run_end.metrics`), plus the report's `end_t`, `progress` and `lost_link_returns`. `log_path` saves the event log, gzipped if it ends in `.gz`. |
| `bench(coordinator, seeds="1-50", preset="small", variant="nominal", *, jobs=None, out=None, save_logs=False, decision_timeout=10.0)` | `{"scorecard": ..., "runs": [...]}` for a seeded batch. |
| `suite(coordinator, seeds="1-50", presets=None, variants=None, *, jobs=None, out=None, save_logs=False, decision_timeout=10.0)` | `{"report": ..., "runs": [...]}` across presets × variants (default: all), with resilience. |
| `score(paths)` | `{"scorecards": [...], "resilience": [...]}` from saved event logs and directories of them, and from a bench's `metrics.jsonl` named as a file. |
| `scenario(seed=0, preset="small", variant="nominal")` | The `hello` message a coordinator gets for that run. |

Arguments:

- **`seeds`** is a list such as `"1-50"` or `"1,4,10-20"`, an int, or an iterable of ints.
- **`out`** writes `metrics.jsonl`, `scorecard.json` and, with `save_logs`, `logs/<run_id>.jsonl.gz`. A suite writes one directory per scenario plus `suite.json`.

How coordinators run in a batch:

- **Command lines** run `jobs` seeds at once, by default one per CPU. The results never depend on `jobs`.
- **In-process coordinators** run seeds one after another. Pass a class to get a fresh instance per seed; an object you pass is reused, and each run starts with its `welcome`. `suite()` takes command lines only.

When a coordinator fails, times out or breaks the protocol, `run()` raises `arcadiabench.CoordinatorError`:

- When the run got under way, the error's `result` holds its record, and the log is complete.
- An exception raised by an in-process coordinator is the error's `__cause__`, with its traceback.
- In a batch, a coordinator that fails mid-run is recorded in its run (`reason` `coordinator_error`, with an `error` message) instead.

`decision_timeout` limits each reply. An in-process coordinator is never interrupted, but a late reply ends the run.

Constants:

- `PRESETS` and `VARIANTS`: the scenario catalog's names.
- `STRESS_VARIANTS`: the variants resilience is measured against.
- `PROTOCOL_VERSION`.
- `SCORECARD_SCHEMA`: the JSON Schema of scorecards, run records and reports.
- `__version__`.

The package is typed (`py.typed`).

`python -m arcadiabench` is the full `arcadiabench` command line (`run`, `bench`, `suite`, `score`), and installing the package also installs it as the `arcadiabench` command.

Scoring is described in [`docs/scoring.md`](../docs/scoring.md), and the presets and variants in [`docs/simulator.md`](../docs/simulator.md).

## Example

[`examples/builder.py`](examples/builder.py) is a simple coordinator that builds the pad, one task at a time.

- **The plan:** survey, dig to level, fill low cells, grade, compact, sinter, build the berm from pits dug inside coverage, and verify.
- **Rescues:** it rescues disabled robots by field repair, a tow to the depot for service, or the mobile charger.
- **Crews:** it picks robots by capability, so it runs on every preset.

On Small it finishes the pad in about 1.8 sols, a speed of about 0.25.

```sh
python examples/builder.py --seed 3 --preset medium --variant dust_storm      # in process
arcadiabench run --seed 3 -- python examples/builder.py --serve               # as a subprocess
```

## Tests

`tests/` covers:

- scenarios for every preset and variant;
- in-process and subprocess runs, which give byte-identical logs;
- `serve()` under the real command line;
- batches at different `jobs`;
- suites and resilience;
- rescoring saved logs;
- coordinator errors.

CI (`.github/workflows/python.yml`) builds the wheel and runs them on Python 3.12.
