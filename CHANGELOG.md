# Changelog

All notable changes to ArcadiaBench. Versions follow semver.

## [Unreleased]

### Changed

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
