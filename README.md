# ArcadiaBench

ArcadiaBench: an open, seeded benchmark for coordinating robot crews building on Mars.

ArcadiaBench is a fast, headless simulator of a construction site at Arcadia Planitia. Any team can plug in a coordinator and compare results. Coordinators connect through [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol), newline-delimited JSON over stdio, so they can be written in any language. [ArcadiaOS](https://github.com/Gabe-Soler/arcadiaos) is the reference coordinator.

**Version 1.1.0, in development on `main`**, speaking arcadia-protocol 1.1.0. The latest release is 1.0.1 (`pip install arcadiabench`, tag `v1.0.1`), which speaks protocol 1.0.0; the two protocol versions do not work together, so use the release with a 1.0 coordinator such as ArcadiaOS 1.0. A coordinator builds a landing pad end to end while robots wear out, break down and get stuck, on a choice of power systems, with Earth minutes away behind a link that drops out, and authority rules that Earth enforces and updates. Crews come in three sizes, every scenario runs across seeds in parallel, and scorecards rank coordinators by success rate × speed. Scenarios come in two catalogs: 1.1, the default, on sourced rates where the pad takes weeks, and 1.0, where it takes hours. Three baseline coordinators ship with the benchmark, the [v1.0 results](results/v1.0/RESULTS.md) compare them with ArcadiaOS across the whole of catalog 1.0, a [replay viewer](viewer/README.md) turns any run into a shareable visual demo, and Python researchers can drive it all through the [`arcadiabench` package](python/README.md).

## Quickstart

With Rust (stable) installed, from the repository root:

```sh
cargo build --release

# The null coordinator does nothing: its robots drain and the run ends fleet_lost after about 58 sols.
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator

# The greedy baseline builds the pad on the Medium crew through a dust storm: about 30 sols, simulated in a second or two.
./target/release/arcadiabench run --seed 42 --preset medium --variant dust_storm -- ./target/release/baseline-greedy

# Ten seeds in parallel, scored: a scorecard and a readable table.
./target/release/arcadiabench bench --preset small --variant nominal --seeds 1-10 --out runs/greedy-small -- ./target/release/baseline-greedy

# The same on catalog 1.0, the one the published v1.0 results are on: builds of hours.
./target/release/arcadiabench bench --catalog 1.0 --preset small --variant nominal --seeds 1-10 --out runs/greedy-small-1.0 -- ./target/release/baseline-greedy
```

Each run prints a one-line summary, and `bench` prints its scorecard. To benchmark ArcadiaOS, check it out beside this repository, build it with `cargo build --release`, and pass `../arcadiaos/target/release/arcadiaos` as the coordinator. To watch a run, open its log in the [replay viewer](#replay-viewer).

## What it simulates

- **Scenario catalogs:** a scenario is a preset (the crew and its outpost) under a variant (what the run throws at it), in a catalog (the robots, crews and every parameter). There are three Standard track presets, Small, Medium and Large, and six variants: `nominal`, `dust_storm`, `conjunction`, `cascading_faults`, `solar_only` and `nuclear_heavy`. Every preset combines with every variant (see [Catalogs, presets and variants](#catalogs-presets-and-variants)).
  - **Catalog 1.1,** the default (`landing-pad-1.1-<preset>-<variant>`): sourced, realistic rates. Crews of 6, 12 and 20; a pad that takes 18 to 55 sols; an Earth link that works in pass windows; answers from Earth in hours; a 100-sol limit. It is being calibrated on `main` and is not final until 1.1.0 is released.
  - **Catalog 1.0** (`--catalog 1.0`, `landing-pad-<preset>-<variant>`): the catalog of the 1.0 release and its published results. Crews of 4, 10 and 20; a pad that takes hours; a continuous link; a 60-sol limit. It is frozen.
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
- **The Earth link:** a one-way delay of minutes from a smooth orbital model (about 12 minutes and slowly growing by default), seeded unplanned outages lasting hours, and in the `conjunction` variant a 14-sol solar-conjunction blackout. In catalog 1.1 the link works in pass windows: a direct window of a few hours and three to five short relay passes each sol, with the link down for about 21 hours in between. Messages are stored and forwarded both ways, with exact delivery times in the log, and `comms` events report every change of link state and every 60 s of drift in the delay.
- **Scripted mission control:** it answers every approval request after a human response time of about 4 hours (about 20 minutes in catalog 1.0), approving with odds that depend on the action, and replies even when its answer will be too late. A deterministic trust script updates the authority policy: it loosens towing and replanning after a run of incident-free sols, and tightens cannibalizing to level 4 once a robot is lost.
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
- **Batch runs:** `arcadiabench bench` and `suite` run seeds in parallel coordinator processes and write per-run metrics, scorecards and gzipped logs. `arcadiabench score` rebuilds scorecards from saved logs or a bench's run records. Results are byte-identical at any `--jobs`.
- **Baseline coordinators:** `baseline-random`, `baseline-greedy` and `baseline-fcfs` are honest protocol clients that share a task decomposition and safety behaviours and differ only in how they allocate tasks to robots (see [Baselines](#baselines)).
- **Results:** ArcadiaOS and the baselines on every scenario over seeds 1–50, with four demo replays (see [Results](#results)).
- **Replay viewer:** a static web page that replays any run's event log (see [Replay viewer](#replay-viewer)).
- **Python bindings:** the `arcadiabench` package (PyO3 + maturin) runs Python coordinators in process or as subprocesses, benches, scores and generates scenarios (see [Python](#python)).
- **Discrete-event engine:** integer-millisecond clock and a deterministic event queue. Robot actions take durations and energy drawn from seeded distributions; there are no per-tick physics.
- **Landing-pad task graph, enforced as physics:** each pad cell goes natural → cleared (dig) → graded → compacted → stabilized (sinter). Out-of-order work is rejected, and reworking a cell resets it. Material is conserved: dig spoil fills the robot's payload and then a stockpile, and haul moves real mass.
- **Verification:** the run ends `goals_complete` only when the pad meets spec (surface state, ±5 cm, ≤2° slope, berm height) **and** every pad cell has been scanned since it last changed.
- **Site power:** a fission baseline plus solar (day/night curve × dust transmittance) feeding a site battery bank, with an always-on essential load served first. Station charging draws from the grid and is throttled when the bank runs dry. Mobile chargers carry their own tank. Hybrid is the default; `solar_only` and `nuclear_heavy` are variants.
- **Environment:** sol clock, temperature cycle, and a seeded dust-storm schedule. The coordinator hears about a storm when it starts, peaks and ends, and at every sunrise while it lasts.
- **Fair comparisons:** terrain, weather and the Earth link schedule come from their own seeded streams, so every coordinator on the same seed faces the same world, and so does every preset. The whole run is reproducible byte for byte.
- **Coordinator harness:** every message is schema-validated, there's a per-decision wall-clock timeout (`--decision-timeout`, default 10 s), and a guard ends the run if a coordinator freezes sim time.
- **Run event log:** JSONL (`event_log.schema.json`) with ground-truth robot snapshots, changed cells and site state at every decision point and at the very end. It is the input for scoring and for the replay viewer.

All tunables live in `src/sim/params.rs`, and [`docs/simulator.md`](docs/simulator.md) explains each one. Catalog 1.0's values are plausible placeholders. Catalog 1.1 uses a published value wherever one exists, and §19 of that document cites each. The interface semantics the simulator implements are in [arcadia-protocol's `docs/semantics.md`](https://github.com/Gabe-Soler/arcadia-protocol/blob/v1.1.0/docs/semantics.md). The constants a coordinator plans with, such as the grade window and the compaction settlement, arrive in `hello` as `scenario.constants`.

## Catalogs, presets and variants

Every scenario has the same site, pad and base station, and within a catalog a seed gives the same terrain, weather and Earth link in every scenario. `--catalog` chooses the catalog; without it a run is on 1.1.

| Preset | Catalog 1.1 crew | Catalog 1.0 crew | Outpost |
|---|---|---|---|
| `small` | 6: a scout, an excavator, a hauler and 3 compactor-sinterers | 4: one of each design | 1 charge station (2 slots), 1 depot |
| `medium` | 12: 2 scouts, 2 excavators, 2 haulers, 6 compactor-sinterers | 10: 2, 2, 2 and 4 | 2 stations, 2 depots |
| `large` | 20: 3 scouts, 4 excavators, 3 haulers, 10 compactor-sinterers | 20: 4, 4, 4 and 8 | 4 stations, 4 depots |

Within a catalog every robot is one of the same four designs, so results differ only by crew size and coordination. Sintering dominates the work: in catalog 1.1 a square metre takes about 7 hours and 37 kWh, so half of every crew sinters and a sinterer spends a fifth of its time charging. Medium and Large carry every capability on at least two robots.

| Variant | What it throws at the crew |
|---|---|
| `nominal` | Hybrid power and the nominal fault model. |
| `dust_storm` | A severe regional storm early in the run. In catalog 1.1 its front arrives in the first three sols, takes two sols to peak and stays for weeks; in catalog 1.0 it arrives in the first half hour and lasts days. Sensor faults, faster wear, dim sun. |
| `conjunction` | A 14-sol blackout of the Earth link, with the delay at its longest. It starts one to four sols into the run in catalog 1.1, and 15–45 minutes in on catalog 1.0. |
| `cascading_faults` | Four to five times the wear and failure rates, and fewer faults fixable in the field. |
| `solar_only` | No fission: day-night swings and storms, with banking and rationing. |
| `nuclear_heavy` | Mostly fission: steady supply against a fixed capacity limit. |

[`docs/simulator.md`](docs/simulator.md) §1 gives catalog 1.0's crews, outposts and variants and the reasoning behind them; §19 gives everything catalog 1.1 changes, with its sources.

### The fleet design track

On the Standard track the crew is given. On the fleet design track the coordinator brings its own: a crew file with a count for each of the catalog's four designs takes the place of the preset.

```sh
echo '{"scout": 1, "excavator": 2, "hauler": 1, "compactor_sinterer": 6}' > crew.json
arcadiabench suite --crew crew.json --seeds 1-50 -- path/to/coordinator
```

- **The rules:** 4 to 20 robots, and no more launch mass than the catalog's budget: 10 t on catalog 1.1, where a compactor-sinterer weighs 900 kg, and 6 t on catalog 1.0. A crew outside them is refused with the reason. Each budget is a little under the mass of its catalog's Large crew.
- **The same for everyone:** every crew works from the Medium outpost (2 charge stations, 2 depots) and meets the same site, weather and Earth link as the presets do on that seed.
- **One reference for every crew:** speed is measured against the time the pad would take with as many sinterers as the budget carries (11 on catalog 1.1, which is 10.2 sols), not against the crew's own. A crew with few sinterers therefore scores lower than one with many, and the choice of crew is part of what is scored.
- **Names:** the scenario is `landing-pad-1.1-crew.1.2.1.6-<variant>`, the counts in the order scout, excavator, hauler, compactor-sinterer. Scorecards carry the crew, and each crew has its own resilience.

[`schemas/crew.schema.json`](schemas/crew.schema.json) describes the crew file.

## Run

```sh
cargo build --release
./target/release/arcadiabench run --seed 42 --log run.jsonl -- ./target/release/null-coordinator
./target/release/arcadiabench run --seed 42 --preset medium --variant solar_only --log run.jsonl.gz -- path/to/arcadiaos
```

Everything after `--` is the coordinator command line. The coordinator's stderr is passed through. A log whose name ends in `.gz` is gzipped. In release builds a full 60-sol run on catalog 1.0 takes well under a second, and a whole build of weeks on catalog 1.1 a second or two. The null coordinator never charges, so its idle robots drain and its run ends `fleet_lost`, after about 58 sols on catalog 1.1 and 17 on catalog 1.0.

## Benchmark

```sh
# One scenario over seeds 1-50, in parallel: a scorecard and a readable table.
arcadiabench bench --preset small --variant nominal --seeds 1-50 --save-logs -- path/to/coordinator
# Every preset × variant, with resilience.
arcadiabench suite --seeds 1-50 -- path/to/coordinator
# Scorecards from saved logs (files or directories); --json for the report.
arcadiabench score results/1.1-small-nominal/logs
```

- **`bench`** writes `results/<catalog>-<preset>-<variant>/` (or `--out DIR`). Catalog 1.0's directories have no catalog in their name: `results/<preset>-<variant>/`.
  - `metrics.jsonl`: one run record per seed.
  - `scorecard.json`.
  - With `--save-logs`, every event log as `logs/<run_id>.jsonl.gz`.
- **`suite`** writes one such directory per scenario under `results/` (or `--out DIR`), plus `suite.json` with every scorecard and the coordinator's resilience.
- **`--crew FILE`** on `run`, `bench` or `suite` takes the place of the preset: [the fleet design track](#the-fleet-design-track). A suite then runs every variant with that crew.
- **Parallelism.** Runs go `--jobs` at a time (default: one per CPU), each with a fresh coordinator process. The outputs are byte-identical at any `--jobs`.
- **`score`** reads `.jsonl` and `.jsonl.gz` event logs, and skips other files in a directory. Named as a file, a bench's `metrics.jsonl` is read too, so runs from several benches can be scored together without their logs.

## Scoring

The leaderboard ranks by one headline metric, **success rate × speed**, with a full scorecard beside it.

- **Speed** of a completed run is `min(1, T_ref / sols)`. `T_ref` is the scenario's reference build time: how long the crew's sinterers would take to sinter the pad, all working at once with nothing else to do. On catalog 1.1 it is 37.5 sols on Small, 18.8 on Medium and 11.3 on Large; on catalog 1.0, 0.451, 0.113 and 0.056. Speed is in (0, 1], twice as fast is twice the speed, and it is comparable across presets because the reference scales with the crew.
- **The headline** is success rate × mean speed, which is the mean over seeds of each run's score (its speed, or 0 if the pad was not finished).
- **The scorecard** adds:
  - completion: success rate, mean and median sols;
  - quality: pad cells in spec, RMS and worst error, slope;
  - energy efficiency: energy per completed pad, lowest storage fraction, deficit time;
  - fleet health: faults, robot-hours lost, robots lost;
  - autonomy: approval requests, time waiting for Earth, policy violations;
  - the conditions the runs met.
- **Resilience** is, per preset, the mean over `dust_storm`, `conjunction` and `cascading_faults` of each one's headline over the nominal headline, capped at 1. A suite averages it over presets. It is worked out within one catalog, never across two, and a fleet design track crew has its own.

[`docs/scoring.md`](docs/scoring.md) gives every formula and the reasoning behind it. [`schemas/scorecard.schema.json`](schemas/scorecard.schema.json) (draft 2020-12) describes run records, scorecards and reports.

## Baselines

Three baseline coordinators ship with the benchmark so results have a reference, as the project outline asks. Build them with `cargo build --release` and run them like any coordinator:

```sh
arcadiabench suite --seeds 1-50 -- ./target/release/baseline-greedy
```

They live in [`src/baselines/`](src/baselines/) and use only `arcadia_protocol`, so they know only what any coordinator could know; `tests/baselines.rs` checks this. They share everything but allocation:

- **Task decomposition.** Each pad cell goes through survey, dig, fill (only if low), grade, compact, sinter and verify, one task at a time per cell, so cells can be at different stages and every robot can work at once. Dig spoil is cast onto low pad cells it suits, then onto the berm, then onto stockpiles beside the pad; the berm gets borrow pits inside coverage if the pad's spoil falls short.
- **Safety.** A robot charges to full, at the station with the shortest trip and queue, when its battery falls below 20% or when it cannot afford the work waiting for it. A fault is repaired in the field by a repair-capable robot when it can be; otherwise the robot is towed to a depot and serviced, or left out of the work if nobody can help. A flat battery gets a mobile charger or a tow to a station.
- **Coverage and authority.** All work stays in the base station's coverage, and a command refused `out_of_coverage` is retried after a pause. Every level-2 command gets a `notify` in the decision that issues it, and nothing at level 3 or 4 is ever done, so the baselines commit no policy violations.

They differ only in how ready tasks meet free robots:

| Baseline | Allocation |
|---|---|
| `baseline-random` | Ready tasks in priority order; each goes to a capable free robot drawn from a generator seeded with the run's seed. |
| `baseline-greedy` | Ready tasks in priority order; each goes to the capable free robot with the lowest estimated cost, travel time plus work time. |
| `baseline-fcfs` | First come, first served: robots in the order they became free; each takes the oldest ready task it can do. |

Priority order is rescans and surveys, then verification scans, then the pad pipeline from its downstream end (sinter, compact, grade, fill, dig), then the berm; within a class, cells in sweep order. A robot only takes a task its battery covers, with the trip to a station after it. All three are deterministic.

## Results

[`results/v1.0/RESULTS.md`](results/v1.0/RESULTS.md) has the benchmark results of ArcadiaBench 1.0, on catalog 1.0: ArcadiaOS 1.0 and the three baselines on all 18 scenarios over seeds 1–50, 3,600 runs, with the headline table, every scorecard, resilience and the scripts that reproduce them byte for byte. In short, ArcadiaOS finishes the pad 15% faster than greedy on the nominal build and has the best headline in every scenario.

| Nominal headline, catalog 1.0 | Small (4 robots) | Medium (10) | Large (20) |
|---|---|---|---|
| ArcadiaOS | **0.559** | **0.753** | **0.710** |
| greedy | 0.501 | 0.681 | 0.585 |
| random | 0.499 | 0.677 | 0.587 |
| first come, first served | 0.385 | 0.419 | 0.362 |

Results on catalog 1.1 will be published with the 1.1.0 release, once ArcadiaOS has run on it. With the baselines alone on seeds 1–50, Small takes about 55 sols, Medium 28 and Large 18, and their nominal headlines are 0.61 to 0.69.

[`docs/demos.md`](docs/demos.md) describes the four demo replays (a nominal build, a dust storm, a conjunction blackout and cascading faults) and how to open them in the replay viewer. `scripts/results.sh` and `scripts/demos.sh` regenerate the results and the replays.

## Replay viewer

[`viewer/`](viewer/README.md) is a static web page that replays a run from its event log: the robots at work, the pad taking shape, site power through dust storms, and the Earth link through outages and blackouts. It has no build step and no dependencies. From the repository root:

```sh
python3 -m http.server 8000
# then open http://localhost:8000/viewer/?log=../results/v1.0/replays/landing-pad-large-nominal-1.jsonl.gz
```

Any log saved with `--log`, or with `--save-logs` in a batch, opens the same way, or from the page's **Open log** button. The **Viewer Pages** workflow publishes the viewer and the results to GitHub Pages whenever either changes on `main`.

## Python

```sh
pip install arcadiabench
```

Wheels cover CPython 3.9 and later on Linux (x86_64 and aarch64), macOS (Intel and Apple silicon) and Windows (x64); elsewhere pip builds the package from source, which needs Rust.

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

## Documentation

- [`docs/simulator.md`](docs/simulator.md): every parameter, the scenario catalog, and the functional forms behind the interface.
- [`docs/scoring.md`](docs/scoring.md): every score, the scorecard and resilience, and why.
- [`docs/demos.md`](docs/demos.md): the four demo replays.
- [`viewer/README.md`](viewer/README.md) and [`python/README.md`](python/README.md): the replay viewer and the Python package.
- [arcadia-protocol](https://github.com/Gabe-Soler/arcadia-protocol/tree/v1.1.0): the message schemas and the normative semantics.
- [`CHANGELOG.md`](CHANGELOG.md): what each release contains.

## Develop

```sh
cargo test     # unit tests, the scripted builder, the baselines, faults and repair, energy, comms and authority, coverage, the catalogs, the fleet design track, scoring and batches, the published results, determinism, regression, harness robustness
cargo test --release --test baselines    # also a whole catalog 1.1 build, which the debug run skips
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
node --test viewer/test/    # the replay viewer, with Node 24
```

The Python package builds separately:

```sh
cd python && ~/.local/bin/uv venv .venv && ~/.local/bin/uv pip install --python .venv maturin pytest
.venv/bin/maturin develop --uv --release && .venv/bin/python -m pytest
```

CI runs all of these on every push and pull request. The **Python release** workflow (`.github/workflows/release-python.yml`) builds the wheels and the source distribution on every push that changes the package's sources, and installs and tests each on its own platform; a pushed `v*` tag that matches the version also publishes them to PyPI.

The baselines are the proof that the parameters make the mission buildable. On catalog 1.0 each of them finishes all 50 seeds of every scenario. On catalog 1.1 they finish all but one of their 2,700 runs: the greedy baseline loses its whole fleet once, under `cascading_faults` on Small. `tests/support/builder.rs` is an older, naive scripted coordinator that builds the pad one task at a time, in about 2 sols, using only what it observes and repairing, towing, servicing and recharging robots as faults strike. The regression tests pin its runs. With faults turned off it takes about 1.7 sols.

## License

MIT
