# ArcadiaBench

ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.

ArcadiaBench is a fast, headless simulator of a construction site at Arcadia Planitia. Any team can plug in a coordinator and compare results. Coordinators connect through [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol), newline-delimited JSON over stdio, so they can be written in any language. [ArcadiaOS](https://github.com/Gabe-Soler/arcadiaos) is the reference coordinator.

## Status

**Week 7: scoring, presets and Python.** A coordinator can build a landing pad end to end while robots wear out, break down and get stuck, on a choice of power systems, with Earth minutes away behind a link that drops out, and authority rules that Earth enforces and updates. Crews come in three sizes, every scenario runs across seeds in parallel, and scorecards rank coordinators by success rate × speed. Python researchers can drive it all through the `arcadiabench` package.

- **Scenario catalog:** `landing-pad-<preset>-<variant>`. Three Standard track presets: Small (4 robots), Medium (10) and Large (20). Six variants: `nominal`, `dust_storm`, `conjunction`, `cascading_faults`, `solar_only` and `nuclear_heavy`. Every preset combines with every variant (see [Presets and variants](#presets-and-variants)).
- **The site:** a seeded 60 × 60 m site (1 m cells) on a gently tilted plain, with variable regolith and a sparse boulder field. The goal is a 20 × 20 m pad with a 12 m blast berm. Crews are defined purely by capabilities and work from charge stations, depots and a comms base station. The coordinator starts with noisy orbital elevation data; scans reveal the ground truth, with sensor noise.
- **Wear and faults:** every robot carries hidden wear on six components (wheels, drive motor, joints, tool, sensors, battery).
  - Wear grows with distance, work and dust, faster in storms, and makes a robot slower and hungrier.
  - Noisy health telemetry (wheel slip, motor current, dust coverage, temperature) hints at it.
  - Three failure processes run on their own seeded streams: wear-driven hazards that climb steeply with wear, random failures at a base rate, and environmental incidents (sensors knocked out by dust storms, robots stuck in soft regolith).
  - Faults are degraded or disabling, and a disabling one may need the depot.
- **Four ways out:**
  - Field `repair` by a repair-capable robot.
  - `tow` to a depot or station.
  - `service` at the depot, which clears every fault and resets wear.
  - `cannibalize`: strip a dead robot for parts; it is lost for good.

  Cold nights add heater drain that can leave idle robots offline, and an offline robot can be recharged where it stands by a mobile charger. The run ends `fleet_lost` when no robot can ever act again.
- **The Earth link:** a one-way delay of 4–24 minutes from a smooth orbital model (about 12 minutes and slowly growing by default), seeded unplanned outages lasting hours, and in the `conjunction` variant a 14-sol solar-conjunction blackout. Messages are stored and forwarded both ways, with exact delivery times in the log, and `comms` events report every change of link state and every 60 s of drift in the delay.
- **Scripted mission control:** it answers every approval request after a human response time of about 20 minutes, approving with odds that depend on the action, and replies even when its answer will be too late. A deterministic trust script updates the authority policy: it loosens towing and replanning after two incident-free sols, and tightens cannibalizing to level 4 once a robot is lost.
- **Authority enforcement:** level-3 and level-4 commands need an approval Earth has granted (or, at level 3, a request whose deadline passed with fallback `proceed`); otherwise they are rejected `approval_required` or `vetoed`. Level-2 commands without a timely `notify` count as policy violations.
- **Coverage and relays:** the base station covers the outpost and the pad but not the blast berm. Relays extend coverage transitively. Robots out of coverage:
  - reject new commands;
  - keep working;
  - hold their reports back until they return;
  - show a stale snapshot.

  An idle robot stranded out of coverage drives back on its own after a timeout.
- **Run metrics:** every log ends with `run_end.metrics`:
  - completion and sols;
  - pad quality;
  - energy (robot and grid energy, lowest storage, deficit time);
  - fleet health (faults, robot-hours lost, robots lost);
  - autonomy (Earth traffic, time waiting for approvals, policy violations);
  - conditions (storm, blackout and outage time).

  Every `truth` record carries the true state of the site's power grid, sky and link.
- **Scoring:** per-run speed and score, scorecards over seeds with the headline success rate × speed, and resilience across stress variants (see [Scoring](#scoring)).
- **Batch runs:** `arcadiabench bench` and `suite` run seeds in parallel coordinator processes and write per-run metrics, scorecards and gzipped logs. `arcadiabench score` rebuilds scorecards from saved logs. Results are byte-identical at any `--jobs`.
- **Python bindings:** the `arcadiabench` package (PyO3 + maturin) runs Python coordinators in process or as subprocesses, benches, scores and generates scenarios (see [Python](#python)).
- **Discrete-event engine:** integer-millisecond clock and a deterministic event queue. Robot actions take durations and energy drawn from seeded distributions; there are no per-tick physics.
- **Landing-pad task graph, enforced as physics:** each pad cell goes natural → cleared (dig) → graded → compacted → stabilized (sinter). Out-of-order work is rejected, and reworking a cell resets it. Material is conserved: dig spoil fills the robot's payload and then a stockpile, and haul moves real mass.
- **Verification:** the run ends `goals_complete` only when the pad meets spec (surface state, ±5 cm, ≤2° slope, berm height) **and** every pad cell has been scanned since it last changed.
- **Site power:** a fission baseline plus solar (day/night curve × dust transmittance) feeding a site battery bank, with an always-on essential load served first. Station charging draws from the grid and is throttled when the bank runs dry. Mobile chargers carry their own tank. Hybrid is the default; `solar_only` and `nuclear_heavy` are variants.
- **Environment:** sol clock, temperature cycle, and a seeded dust-storm schedule. The coordinator hears about a storm when it starts, peaks and ends, and at every sunrise while it lasts.
- **Fair comparisons:** terrain, weather and the Earth link schedule come from their own seeded streams, so every coordinator on the same seed faces the same world, and so does every preset. The whole run is reproducible byte for byte.
- **Coordinator harness:** every message is schema-validated, there's a per-decision wall-clock timeout (`--decision-timeout`, default 10 s), and a guard ends the run if a coordinator freezes sim time.
- **Run event log:** JSONL (`event_log.schema.json`) with ground-truth robot snapshots, changed cells and site state at every decision point and at the very end. It is the input for scoring and for the replay viewer.

**Not yet:** the random, greedy and first-come-first-served baseline coordinators, and full results across the catalog (week 9).

All tunables live in `src/sim/params.rs`, and [`docs/simulator.md`](docs/simulator.md) explains each one. They are plausible placeholders, and sourcing realistic values is still open. The interface semantics the simulator implements are in [arcadia-protocol's `docs/semantics.md`](https://github.com/Gabe-Soler/arcadia-protocol/blob/v0.2.0/docs/semantics.md).

## Presets and variants

Every scenario has the same site, pad and base station, and a seed gives the same terrain, weather and Earth link in every scenario.

| Preset | Crew | Outpost |
|---|---|---|
| `small` | 4: one scout, excavator, hauler and compactor-sinterer | 1 charge station (2 slots), 1 depot |
| `medium` | 10: 2 scouts, 2 excavators, 2 haulers, 4 compactor-sinterers | 2 stations, 2 depots |
| `large` | 20: 4 scouts, 4 excavators, 4 haulers, 8 compactor-sinterers | 4 stations, 4 depots |

Every robot is one of the same four designs, so results differ only by crew size and coordination. Sintering dominates the work, so beyond Small the compactor-sinterers make up 40% of the crew. Medium and Large carry every capability on at least two robots. The crews nest: Medium is the Small crew plus six robots, and Large is the Medium crew plus ten.

| Variant | What it throws at the crew |
|---|---|
| `nominal` | Hybrid power and the nominal fault model. |
| `dust_storm` | A severe regional storm arriving in the first 0.1 sol: sensor faults, faster wear, days of dim sun. |
| `conjunction` | A 14-sol blackout of the Earth link early in the run, with the delay at its longest. |
| `cascading_faults` | Four to five times the wear and failure rates, and fewer faults fixable in the field. |
| `solar_only` | No fission: day-night swings and storms, with banking and rationing. |
| `nuclear_heavy` | Mostly fission: steady supply against a fixed capacity limit. |

[`docs/simulator.md`](docs/simulator.md) §1 gives every preset's crew and outpost, the reasoning behind the capability mix, and every variant's parameters.

## Run

```sh
cargo build --release
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator
./target/release/arcadiabench run --seed 42 --preset medium --variant solar_only --log run.jsonl.gz -- path/to/arcadiaos
```

Everything after `--` is the coordinator command line. The coordinator's stderr is passed through. A log whose name ends in `.gz` is gzipped. A full 60-sol run takes well under a second in release builds. The null coordinator never charges, so its idle robots drain and its run ends `fleet_lost` after about 17 sols.

## Benchmark

```sh
# One scenario over seeds 1-50, in parallel: a scorecard and a readable table.
arcadiabench bench --preset small --variant nominal --seeds 1-50 --save-logs -- path/to/coordinator
# Every preset × variant, with resilience.
arcadiabench suite --seeds 1-50 -- path/to/coordinator
# Scorecards from saved logs (files or directories); --json for the report.
arcadiabench score results/small-nominal/logs
```

- **`bench`** writes `results/<preset>-<variant>/` (or `--out DIR`):
  - `metrics.jsonl`: one run record per seed.
  - `scorecard.json`.
  - With `--save-logs`, every event log as `logs/<run_id>.jsonl.gz`.
- **`suite`** writes one such directory per scenario under `results/` (or `--out DIR`), plus `suite.json` with every scorecard and the coordinator's resilience.
- **Parallelism.** Runs go `--jobs` at a time (default: one per CPU), each with a fresh coordinator process. The outputs are byte-identical at any `--jobs`.
- **`score`** reads `.jsonl` and `.jsonl.gz` event logs, and skips other files in a directory.

## Scoring

The leaderboard ranks by one headline metric, **success rate × speed**, with a full scorecard beside it.

- **Speed** of a completed run is `min(1, T_ref / sols)`. `T_ref` is the scenario's reference build time: how long the crew's sinterers would take to sinter the pad, all working at once with nothing else to do. It is 0.451 sols on Small, 0.113 on Medium and 0.056 on Large. Speed is in (0, 1], twice as fast is twice the speed, and it is comparable across presets because the reference scales with the crew.
- **The headline** is success rate × mean speed, which is the mean over seeds of each run's score (its speed, or 0 if the pad was not finished).
- **The scorecard** adds:
  - completion: success rate, mean and median sols;
  - quality: pad cells in spec, RMS and worst error, slope;
  - energy efficiency: energy per completed pad, lowest storage fraction, deficit time;
  - fleet health: faults, robot-hours lost, robots lost;
  - autonomy: approval requests, time waiting for Earth, policy violations;
  - the conditions the runs met.
- **Resilience** is, per preset, the mean over `dust_storm`, `conjunction` and `cascading_faults` of each one's headline over the nominal headline, capped at 1. A suite averages it over presets.

[`docs/scoring.md`](docs/scoring.md) gives every formula and the reasoning behind it. [`schemas/scorecard.schema.json`](schemas/scorecard.schema.json) (draft 2020-12) describes run records, scorecards and reports.

## Python

```python
import arcadiabench

class Idle(arcadiabench.Coordinator):
    name = "idle"

    def decide(self, observation):
        return self.decision(observation)  # do nothing

record = arcadiabench.run(Idle(), seed=1, preset="small", variant="nominal")
batch = arcadiabench.bench(["path/to/coordinator"], seeds="1-50", preset="medium", jobs=8)
print(batch["scorecard"]["headline"])
```

- **Coordinators.** A coordinator is a command line, run as a subprocess, or a Python object with `welcome`, `decide` and `end`, run in process under the same schema checks. `arcadiabench.serve(coordinator)` runs a Python coordinator under the command line: `arcadiabench run -- python my_coordinator.py`.
- **Example.** `python/examples/builder.py` is a simple coordinator that builds the pad.
- **More.** [`python/README.md`](python/README.md) covers setup, the API and the example.

## Develop

```sh
cargo test     # unit tests, the scripted builder, faults and repair, energy, comms and authority, coverage, the catalog, scoring and batches, determinism, regression, harness robustness
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

The Python package builds separately:

```sh
cd python && ~/.local/bin/uv venv .venv && ~/.local/bin/uv pip install --python .venv maturin pytest
.venv/bin/maturin develop --uv --release && .venv/bin/python -m pytest
```

`tests/support/builder.rs` is a naive scripted coordinator that builds the pad in about 2 sols using only what it observes, repairing, towing, servicing and recharging robots as faults strike. It keeps all its work in coverage and notifies Earth of every level-2 command. It's the proof that the default parameters make the mission buildable, through a conjunction and a dust storm, and on every preset. With faults turned off it takes about 1.7 sols.

## License

MIT
