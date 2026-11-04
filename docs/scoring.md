# Scoring

ArcadiaBench ranks coordinators by one headline metric, **mission success rate × speed**, and publishes a full scorecard beside it, as the project outline asks. Every number is computed from the `run_end.metrics` of each run (semantics §11), so a score can always be rebuilt from saved event logs. The code is in [`src/score/`](../src/score/), and [`schemas/scorecard.schema.json`](../schemas/scorecard.schema.json) describes the JSON.

Scoring works at three levels:

1. **A run** gets a speed and a score (§1).
2. **A scorecard** aggregates the runs of one coordinator on one scenario over many seeds. Its headline is success rate × mean speed (§2), and its full scorecard covers completion, quality, energy efficiency, fleet health, autonomy and conditions (§3).
3. **Resilience** compares a coordinator's stress variants with its own nominal runs, across a suite (§4).

## Contents

1. [A run: reference time, speed and score](#1-a-run-reference-time-speed-and-score)
2. [The headline](#2-the-headline)
3. [The scorecard](#3-the-scorecard)
4. [Resilience](#4-resilience)
5. [Seeds, determinism and files](#5-seeds-determinism-and-files)

## 1. A run: reference time, speed and score

### Reference build time

Each scenario has a **reference build time** `T_ref`, in sols:

```
T_ref = max over c of ( A / R_c ) / sol_length_s
```

Here:

- `A` is the pad's area: cells × `cell_size_m`².
- `c` ranges over the per-area treatments every pad cell needs before the pad is done: grade, compact, sinter (when the pad is stabilized) and a verification scan.
- `R_c` is the crew's total nominal rate for `c`: the sum of the `rate` of that capability over every robot that has it.

`T_ref` is how long the crew would need to give the whole pad its slowest treatment, with every robot that can do it working at once at its nominal rate, and nothing else: no travel, charging, waiting, wear or other work. For every preset that treatment is sintering, so `T_ref = A / R_sinter`:

| Preset | Sinterers | `R_sinter` | `T_ref` |
|---|---|---|---|
| `small` | 1 | 0.01 m²/s | 40,000 s = 0.451 sols |
| `medium` | 4 | 0.04 m²/s | 10,000 s = 0.113 sols |
| `large` | 8 | 0.08 m²/s | 5,000 s = 0.056 sols |

`T_ref` is computed from the scenario the coordinator receives, so it is defined for any crew, including crews outside the catalog. If the crew lacks a treatment the pad needs, no run can finish, and `T_ref` is null.

### Speed

A run that completes the pad in `sols` has

```
speed = min(1, T_ref / sols)
```

- **Bounded in (0, 1].** `sols` is positive, and the cap keeps speed at most 1.
- **Monotonic.** A faster run never has a lower speed.
- **Scale-free.** Twice as fast is twice the speed, at any scale, so ratios of speeds are ratios of build rates. That is what lets resilience (§4) compare a slowed-down run with its nominal one fairly on every preset.
- **Comparable across presets.** `T_ref` shrinks in proportion as a preset adds sinterers, so speed measures how well a coordinator uses the crew it has, not how big that crew is. A coordinator that keeps a Large crew as busy as a Small one gets the same speed on both.

`T_ref` is a lower bound on any real build, apart from duration noise: a step's duration carries ±15% lognormal noise, so a perfect coordinator might beat `T_ref` by a few percent, and the cap absorbs that. No real build comes close. The pipeline has to fill before sintering can start, the Small compactor-sinterer needs about 5 hours of charging, and robots must drive. ArcadiaOS as of week 5 reaches a speed of about 0.56 on Small and 0.41 on Medium and Large.

**Why not the time limit?** Speed could be measured against the 60-sol time limit, for example `1 − sols / 60`. That is comparable across presets, but builds take 0.1 to 2 sols, so every competent coordinator would score between 0.97 and 1.00, and a coordinator twice as fast as another would gain only about 1%. The headline would be the success rate in disguise. The outline's typical result, "finishes the pad X% faster than greedy", needs a speed that resolves those differences.

**Why not a fixed time scale?** A form such as `1 / (1 + sols)` resolves differences, but it is not scale-free. Halving a Large crew's pace would cost it a different fraction of its score than halving a Small crew's, and Large would score higher just for being bigger.

### Score

```
score = speed   if the run completed (it ended goals_complete)
score = 0       otherwise
```

Failure scores zero, whatever the reason: `time_limit`, `fleet_lost` or `coordinator_error`. The score is nearly continuous at the deadline, since a run finishing at the 60-sol limit would have a speed of only about 0.008 on Small.

Every run record (a line of `metrics.jsonl`) carries `reference_sols`, `speed` (null for an incomplete run), `score` and the run's `metrics`.

## 2. The headline

For a coordinator on one scenario over `N` seeds:

```
headline = success_rate × mean_speed = (1/N) Σ score_i
```

- `success_rate` is the completed runs over `N`.
- `mean_speed` is the mean speed of the completed runs.
- The two forms are equal. The headline is the expected score of a run on a seed drawn at random, so it is in [0, 1].

Multiplying, rather than adding or ranking lexicographically, prices reliability against pace in the same units. A coordinator that finishes 90% of seeds at speed 0.6 (headline 0.54) beats one that finishes every seed at speed 0.45 (0.45), and loses to one that finishes every seed at 0.6. If no run completes, the headline is 0.

Compare headlines between coordinators on the same scenario and seeds. Across presets, a drop in headline means a coordinator uses a bigger crew less well (see the remark on speed above).

## 3. The scorecard

`scorecard.json` holds the headline and these groups. "All runs" means every seed; "completed runs" means those that ended `goals_complete`. Means are taken in seed order, so the result never depends on the order the runs finished.

| Group | Field | Definition |
|---|---|---|
| | `scenario_id`, `preset`, `variant` | The scenario. `preset` and `variant` are null for a scenario outside the catalog. |
| | `coordinator`, `simulator` | Name and version of each. |
| | `seeds` | The seeds scored, ascending. |
| | `reference_sols`, `headline` | §1 and §2. |
| **Completion** | `runs`, `completed`, `success_rate` | Pad finished or not. |
| | `mean_speed` | Mean speed of the completed runs. |
| | `mean_sols`, `median_sols`, `min_sols`, `max_sols` | Sols taken by the completed runs. |
| | `end_reasons` | How many runs ended each way. |
| **Quality** | `mean_cells_in_spec` | Mean share of pad cells in spec at the end, over all runs: partial credit for unfinished pads. |
| | `verified_rate`, `berm_complete_rate` | Share of runs whose whole pad was verified, and whose berm was complete, at the end. |
| | `mean_rms_error_m` | Mean RMS elevation error of the completed pads. Every completed pad is within tolerance, so this measures how far inside it the build landed. |
| | `max_abs_error_m`, `max_slope_deg` | The worst cell and the steepest slope on any completed pad. |
| **Energy efficiency** | `energy_per_pad_wh` | Energy drawn from robot batteries in all runs, divided by the pads completed. Unfinished runs count against it. Null if no pad was completed. |
| | `mean_robot_energy_wh`, `mean_grid_energy_wh` | Per run. |
| | `min_storage_fraction` | The lowest site storage in any run, as a fraction of capacity: the closest approach to a power deficit. |
| | `mean_min_storage_fraction`, `mean_deficit_s` | Per run. |
| **Fleet health** | `mean_faults`, `mean_robot_hours_lost`, `mean_robots_lost` | Per run. |
| | `robots_lost` | Robots lost over all runs. |
| **Autonomy** | `mean_approval_requests` | Requests sent to Earth, per run. |
| | `mean_downlink_messages`, `mean_notifications`, `mean_status_reports` | Earth traffic, per run. |
| | `mean_approval_wait_s` | Time spent waiting for Earth's answers, per run. |
| | `mean_policy_violations` | Level-2 commands without a timely notify, per run. |
| **Conditions** | `mean_storm_s`, `mean_blackout_s`, `mean_outage_s` | The adversity the runs met, per run. These are context, not merit. |

Statistics that only make sense for a finished pad, such as sols, speed and pad error, are taken over completed runs. Everything else is over all runs, because a coordinator's failures are part of its behaviour.

A run that fails and goes on to the time limit accumulates idle drain and robot-hours lost for up to 60 sols. That is what the metrics mean (semantics §11), and it shows up in `energy_per_pad_wh` and `mean_robot_hours_lost`.

## 4. Resilience

Resilience is the outline's "performance under dust storms, blackouts and faults". It compares a coordinator's headline under each stress variant with its own nominal headline, on the same preset and seeds:

```
retention(v) = min(1, headline(v) / headline(nominal))     v ∈ {dust_storm, conjunction, cascading_faults}
resilience(preset) = mean of the three retentions
resilience = mean of resilience(preset) over the presets where it is known
```

- **The stress variants** are the outline's three adversities. `solar_only` and `nuclear_heavy` are alternative power architectures, with nuclear-heavy the outline's "easy" mode, so they are scored but are not stresses.
- **Retention is capped at 1.** A coordinator gains nothing from a stress variant that happens to go better than nominal; on a handful of seeds that is noise.
- **Unknown cases.** A retention is null when that variant was not scored or the nominal headline is 0. A preset's resilience is null unless all three retentions are known.
- **Scale-free.** Because speed is scale-free (§1), a run slowed to half its pace keeps half its score on every preset, so retentions are comparable across presets.

`arcadiabench suite` computes resilience for its coordinator. `arcadiabench score` computes it for every coordinator whose logs include a nominal scenario.

## 5. Seeds, determinism and files

- **Seeds.** Every scenario reproduces bit for bit from its seed, and every coordinator on a seed faces the same terrain, weather and Earth link. Leaderboard results use seeds 1–50, the default of `bench` and `suite`.
- **Determinism.** What a batch writes depends only on the coordinator and the seeds, never on `--jobs`: runs are independent and are written and aggregated in seed order. Saved logs are gzipped with a fixed header. A deterministic coordinator therefore gives byte-identical `metrics.jsonl`, `scorecard.json` and logs at any `--jobs`, and `tests/scoring.rs` checks this. The per-decision timeout is wall-clock, so keep `--jobs` at or below the number of CPUs.
- **Files.** `arcadiabench bench` writes into its output directory:
  - `metrics.jsonl`: one run record per line, in seed order (`#/$defs/RunRecord`).
  - `scorecard.json`: the scorecard (the schema's root).
  - With `--save-logs`, `logs/<run_id>.jsonl.gz`: every event log.

  `arcadiabench suite` writes one such directory per scenario, `<preset>-<variant>/`, plus `suite.json`, a report holding every scorecard and the resilience (`#/$defs/Report`). `arcadiabench score --json` prints the same report, built from event logs.
- **Rescoring.** `arcadiabench score` reads only `run_start`, the opening goal and `run_end` from each log, so scorecards rebuilt from saved logs are identical to the ones the bench wrote. The one exception is the `error` text of runs that failed mid-run, which logs do not record.
