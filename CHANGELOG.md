# Changelog

All notable changes to ArcadiaBench. Versions follow semver.

## [Unreleased]

Version 1.1.0, in development. It speaks arcadia-protocol 1.1.0, which does not work with protocol 1.0: use ArcadiaBench 1.0.1 with a 1.0 coordinator.

### Fixed

- **The coverage leak** (semantics §4.1, §8.5; protocol 1.0's "known limitation"). A `repair`, `tow`, `cannibalize` or mobile `charge` that names a robot out of coverage is now checked against that robot's state at last contact, not its true state. The estimate is worked out from that state too, and the executor drives to the cell the robot was last heard in. So a rejection or an estimate no longer tells a coordinator what it has not seen. On arrival the executor finds the robot as it truly is. This changes which commands are accepted in that one case; no pinned run meets it, and every regression fingerprint still matches.

### Changed

- **Protocol 1.1.0.** ArcadiaBench speaks arcadia-protocol 1.1.0. What it simulates on the catalog is unchanged: every regression fingerprint taken on 1.0 still matches once the fields 1.1 adds are cut out of the log.
  - Every `hello` carries `scenario.constants`: the grade window, the repair and towing costs, the depot's service time, the lost-link timeout and the other constants a coordinator plans with.
  - A mobile charger's snapshots carry `tank_wh`, the energy left in its tank.
  - Run metrics carry `coverage` (the commands rejected `out_of_coverage`, and the lost-link returns) and `conditions.between_passes_s`.
  - The baselines read the minimum work duration and the rock density from the scenario's constants.
- **Scorecard schema.** `schemas/scorecard.schema.json` refers to the protocol's 1.1.0 schemas, and its `$id` is now `https://raw.githubusercontent.com/Gabe-Soler/arcadiabench/v1.1.0/schemas/scorecard.schema.json`. Two optional fields are new: `end_reasons.other`, for runs that ended for a reason this version does not know, and `conditions.mean_between_passes_s`. Scorecards written by 1.0 are still valid.
- **Run summary.** `arcadiabench run` reports the sols a run took ("after 0.92 sols") instead of the sol it ended at, and counts the time between passes in "link down".
- **Replay viewer.**
  - Protocol 1.1 logs load without a version notice.
  - A stretch of link down time keeps each of its causes: an outage that gives way to the blackout is shown as an outage, then a blackout. During an outage the link's return is shown as unknown, as it was at the time.
  - The timeline draws the gaps between passes as a faint band, and a blackout or an outage as the full one. A band's label is shortened to fit inside it.
  - The Earth link panel says when the pass window in progress closes, and the banner over the map is kept for blackouts and outages.
  - A mobile charger's details show its charge tank.
  - The time limit is counted from the start of the run.

### Added

- **Pass windows on the Earth link** (semantics §8.1), off in every scenario of the catalog. With `pass_windows` on, the link is up only during each sol's direct-to-Earth window and relay passes, drawn from their own `passes` stream. Between them it is down with cause `between_passes`; `comms` events carry `window_end_t` while it is up and `expected_restore_t` while it waits for the next pass; messages wait for a pass; and the run metrics split the down time by cause.
- **Runs that start at any time of the sol.** `start_sols` draws `scenario.start_t` from the `start` stream. The calendar stays put (sunrise, the seasonal storms, the pass windows), and the run is timed from its start: the time limit, the variant's storm front and blackout, the first outage and the delay's epoch. The catalog keeps starting at midnight.
- **Regression tests.** The baselines' runs on the catalog are fingerprinted, as the test coordinators' already were: 17 runs across the three baselines.

## [1.0.1] - 2026-11-09

The Python package on PyPI, and the release's renamed repository and restyled viewer.

### Added

- **`pip install arcadiabench`.** The Python package is on PyPI: abi3 wheels for CPython 3.9 and later on Linux (x86_64 and aarch64), macOS (universal2) and Windows (x64), and a source distribution for everything else. The new **Python release** workflow builds them on every push that changes the package's sources, installs and tests each on its own platform, and publishes them to PyPI from a version tag, by trusted publishing.

### Changed

- **Viewer on GitHub Pages.** The Viewer Pages workflow now also runs on every push to `main` that changes `viewer/` or `results/`, so the site follows `main` without a manual run.
- **Source distribution.** The crate now lists the files it is built from (`include` in `Cargo.toml`), so the Python source distribution carries the simulator's sources and not the results, the viewer or the tests: 175 KB instead of 5.5 MB.
- **Repository name.** The repository is now `Gabe-Soler/arcadiabench`, and the package metadata and docs link there. The replay viewer's GitHub Pages site follows the name, at `https://gabe-soler.github.io/arcadiabench/`. GitHub still resolves the old `Gabe-Soler/ArcadiaBench` addresses, so the 1.0.0 scorecard schema keeps its `$id`.
- **Replay viewer.** A new look, light and dark: the system's setting picks one, and the page switches live when it changes, canvases included. Display type is Manrope and text is Inter, controls are pills, panels are soft borderless cards, and blue is kept for the primary action. The start screen is a landing page with the samples as cards. What the viewer shows is unchanged.

## [1.0.0] - 2026-11-04

The first release: an open, seeded benchmark for coordinating robot crews building a landing pad at Arcadia Planitia, with its scenario catalog, scoring, baselines, results, replay viewer and Python package.

### Added

- **Protocol.** Speaks arcadia-protocol 1.0.0 and implements every section of its semantics. Every message a coordinator sends is checked against the protocol's JSON Schemas.
- **Simulator.**
  - A discrete-event engine with an integer-millisecond clock, a deterministic event queue and named random streams, so every run reproduces byte for byte from its seed.
  - A seeded site with terrain, regolith and boulders, and a noisy orbital prior; scans reveal the ground truth with sensor noise.
  - Robots defined by capabilities. The landing-pad task graph is enforced as physics, material is conserved, and the goal is complete only once every pad cell is scanned after its last change.
  - Site power: fission, solar and a battery bank, an essential load served first, station charging throttled when the bank runs dry, and mobile chargers.
  - The environment: the sol clock, a temperature cycle and seeded dust storms.
  - Hidden wear on six components, noisy health telemetry, wear-driven, random and environmental failures, getting stuck and cold nights; field repair, towing, depot service and cannibalizing as the ways out.
  - The Earth link: a 4–24 minute light-time delay, unplanned outages and the conjunction blackout, with store-and-forward delivery both ways.
  - Scripted mission control, which answers approval requests and updates the authority policy by a trust script, and authority enforcement at every command.
  - Site coverage, relays and lost-link returns.
  - The run metrics on every `run_end`.
- **Scenario catalog.** Small, Medium and Large crews (the Standard track) under six variants: `nominal`, `dust_storm`, `conjunction`, `cascading_faults`, `solar_only` and `nuclear_heavy`.
- **Harness and logs.** `arcadiabench run` launches any coordinator over stdio, with a per-decision timeout and a guard against frozen sim time. Every run writes a JSONL event log, optionally gzipped, with ground truth for scoring and replay.
- **Scoring.** Speed and score per run, scorecards with the headline success rate × speed, and resilience across the stress variants, all rebuildable from saved logs; `schemas/scorecard.schema.json` describes the JSON.
- **Batch runs.** `arcadiabench bench`, `suite` and `score`: seeds in parallel, byte-identical at any `--jobs`.
- **Baseline coordinators.** `baseline-random`, `baseline-greedy` and `baseline-fcfs`, which share everything but allocation, and `null-coordinator`.
- **Results.** `results/v1.0/`: ArcadiaOS 1.0 and the baselines on all 18 scenarios over seeds 1–50 (3,600 runs), every scorecard, and four demo replays. `scripts/results.sh` and `scripts/demos.sh` reproduce them byte for byte.
- **Replay viewer.** `viewer/`, a static page with no build step that replays protocol 0.1, 0.2 and 1.0 logs, and a workflow that publishes it with the results to GitHub Pages.
- **Python package.** `arcadiabench` 1.0.0 (PyO3 + maturin): runs, batches, suites, scoring and scenarios, with Python coordinators in process or as subprocesses.
- **Documentation.** `docs/simulator.md`, `docs/scoring.md` and `docs/demos.md`, and READMEs for the repository, the viewer and the Python package.
- **CI.** Formatting, lints, tests and rustdoc for the Rust crate; the Python package and its tests; the viewer's tests.

## [0.1.0] - 2026-11-02

The project scaffold.

- A coordinator harness that launches any coordinator over stdio, performs the arcadia-protocol 0.1.0 handshake and schema-validates everything the coordinator sends.
- A JSONL run event log and a placeholder landing-pad scenario.
- The `arcadiabench run` command line and `null-coordinator`.
- End-to-end tests, including determinism and misbehaving coordinators, and CI.
