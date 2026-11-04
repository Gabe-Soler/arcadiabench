# ArcadiaBench v1.0 results

The benchmark results of ArcadiaBench 1.0: ArcadiaOS 1.0 and the three baseline coordinators that ship with ArcadiaBench, on every preset and variant of the catalog, over seeds 1–50. That is 18 scenarios × 50 seeds × 4 coordinators, 3,600 runs in all. Every run completed the pad.

> **ArcadiaOS finishes the pad 15% faster than greedy, and neither needs Earth on the nominal build.**
>
> That is the outline's headline, "X% faster than greedy, with Y fewer Earth interventions", computed on the nominal build and averaged over the three presets: X = 15% and Y = 0, because neither coordinator asks Earth for anything there. See [the headline sentence](#the-headline-sentence).

Contents: [headline](#headline) · [every scenario](#every-scenario) · [resilience](#resilience) · [what the results show](#what-the-results-show) · [the coordinators](#the-coordinators) · [full scorecards](#full-scorecards) · [reproduce](#reproduce) · [files](#files)

## Headline

The headline metric is success rate × mean speed ([`docs/scoring.md`](../../docs/scoring.md)). Speed is the scenario's reference build time over the sols taken, so it is comparable across presets. Nominal variant, 50 seeds per cell:

| Coordinator | Small | Medium | Large |
|---|---|---|---|
| ArcadiaOS | **0.559** | **0.753** | **0.710** |
| greedy | 0.501 | 0.681 | 0.585 |
| random | 0.499 | 0.677 | 0.587 |
| FCFS | 0.385 | 0.419 | 0.362 |

Sols to finish, mean (fastest–slowest):

| Coordinator | Small | Medium | Large |
|---|---|---|---|
| ArcadiaOS | 0.806 (0.788–0.896) | 0.150 (0.144–0.178) | 0.079 (0.077–0.087) |
| greedy | 0.900 (0.880–0.970) | 0.167 (0.155–0.225) | 0.097 (0.091–0.149) |
| random | 0.904 (0.886–0.989) | 0.168 (0.156–0.245) | 0.097 (0.091–0.124) |
| FCFS | 1.174 (1.080–1.472) | 0.271 (0.248–0.418) | 0.156 (0.148–0.172) |

### The headline sentence

The outline's typical headline result reads "ArcadiaOS finishes the pad X% faster than greedy, with Y fewer Earth interventions". Computed from [`results.json`](results.json):

- **X** is the mean over the three presets of ArcadiaOS's nominal headline over greedy's, minus one. Every run of both completed, so this is the ratio of mean speeds: how much faster the pad gets built.
- **Y** is greedy's approval requests per run minus ArcadiaOS's, on the same runs. An approval request is the only message that asks Earth to act: notifies and status reports need no answer (semantics §8.3).

| Preset | ArcadiaOS headline | Greedy headline | Faster | Approval requests per run (ArcadiaOS, greedy) |
|---|---|---|---|---|
| small | 0.559 | 0.501 | 11.7% | 0.00, 0.00 |
| medium | 0.753 | 0.681 | 10.7% | 0.00, 0.00 |
| large | 0.710 | 0.585 | 21.3% | 0.00, 0.00 |

So X = 14.5% and Y = 0: ArcadiaOS builds the pad 15% faster, and neither coordinator needs Earth on the nominal build. Across the whole catalog ArcadiaOS's headline is 10–21% above greedy's in every one of the 18 scenarios. In all 900 of its runs ArcadiaOS asked Earth 2 times, both to cannibalize a robot in Large `cascading_faults`; the baselines never ask, because they never take a level-3 or level-4 action.

## Every scenario

Headline by scenario; the best in each row is in bold. ArcadiaOS has the best headline in all 18.

| Scenario | ArcadiaOS | greedy | random | FCFS |
|---|---|---|---|---|
| `small` / `nominal` | **0.559** | 0.501 | 0.499 | 0.385 |
| `small` / `dust_storm` | **0.543** | 0.456 | 0.453 | 0.349 |
| `small` / `conjunction` | **0.560** | 0.501 | 0.496 | 0.385 |
| `small` / `cascading_faults` | **0.514** | 0.444 | 0.443 | 0.325 |
| `small` / `solar_only` | **0.560** | 0.501 | 0.499 | 0.385 |
| `small` / `nuclear_heavy` | **0.559** | 0.501 | 0.499 | 0.385 |
| `medium` / `nominal` | **0.753** | 0.681 | 0.677 | 0.419 |
| `medium` / `dust_storm` | **0.732** | 0.665 | 0.653 | 0.400 |
| `medium` / `conjunction` | **0.753** | 0.681 | 0.677 | 0.416 |
| `medium` / `cascading_faults` | **0.719** | 0.655 | 0.647 | 0.375 |
| `medium` / `solar_only` | **0.753** | 0.681 | 0.677 | 0.419 |
| `medium` / `nuclear_heavy` | **0.753** | 0.681 | 0.677 | 0.419 |
| `large` / `nominal` | **0.710** | 0.585 | 0.587 | 0.362 |
| `large` / `dust_storm` | **0.701** | 0.580 | 0.581 | 0.356 |
| `large` / `conjunction` | **0.710** | 0.585 | 0.587 | 0.362 |
| `large` / `cascading_faults` | **0.692** | 0.573 | 0.572 | 0.351 |
| `large` / `solar_only` | **0.710** | 0.585 | 0.587 | 0.362 |
| `large` / `nuclear_heavy` | **0.710** | 0.585 | 0.587 | 0.362 |

## Resilience

Resilience is a coordinator's headline under each stress variant over its own nominal headline, capped at 1, averaged over `dust_storm`, `conjunction` and `cascading_faults`, then over the presets ([`docs/scoring.md`](../../docs/scoring.md) §4). The dust storm and the blackout begin within the first 0.03 sol, so they cover every preset's build ([`docs/simulator.md`](../../docs/simulator.md) §1).

| Coordinator | Small | Medium | Large | Suite |
|---|---|---|---|---|
| ArcadiaOS | 0.963 | 0.976 | 0.987 | **0.975** |
| greedy | 0.932 | 0.980 | 0.990 | 0.967 |
| random | 0.931 | 0.974 | 0.989 | 0.964 |
| FCFS | 0.916 | 0.945 | 0.984 | 0.949 |

Retention of each stress variant:

| Coordinator | Preset | `dust_storm` | `conjunction` | `cascading_faults` |
|---|---|---|---|---|
| ArcadiaOS | small | 0.971 | 1.000 | 0.919 |
| ArcadiaOS | medium | 0.973 | 1.000 | 0.955 |
| ArcadiaOS | large | 0.987 | 1.000 | 0.974 |
| greedy | small | 0.910 | 0.999 | 0.887 |
| greedy | medium | 0.978 | 1.000 | 0.962 |
| greedy | large | 0.992 | 1.000 | 0.978 |
| random | small | 0.909 | 0.995 | 0.888 |
| random | medium | 0.965 | 1.000 | 0.957 |
| random | large | 0.990 | 1.000 | 0.976 |
| FCFS | small | 0.905 | 0.999 | 0.844 |
| FCFS | medium | 0.953 | 0.991 | 0.893 |
| FCFS | large | 0.983 | 1.000 | 0.970 |

## What the results show

- **ArcadiaOS wins every scenario.** Its nominal headline is 12% above greedy's on Small, 11% on Medium and 21% on Large. Every coordinator completed all 50 seeds of all 18 scenarios with no policy violation and no robot lost.
- **Where the time goes.** Sintering is the slowest step, and on every preset the build is bound by the compactor-sinterers.
  - *Small.* Its one compactor-sinterer must compact and sinter all 400 cells, about 44,000 s of work, and draw about 26 kWh, three times its battery. So it spends about six hours at the station: most of the gap between ArcadiaOS's 0.81 sols and the 0.45-sol reference, which counts sintering alone. ArcadiaOS charges it to what its remaining work needs; the baselines charge to full whenever a battery falls below 20%.
  - *Medium and Large.* What matters is how much of the sinterers' time goes on driving. Measured on seeds 1–5, compacting and sintering a cell, driving included, takes ArcadiaOS's compactor-sinterers 128 s on Medium and 136 s on Large, and greedy's 136 s and 155 s. Greedy's sinter the cell they have just compacted, but then cross the pad to compact the earliest graded cell in sweep order, 4–7 cells away; ArcadiaOS's take work 1–2 cells away. ArcadiaOS also starts sintering 2–5 minutes into the run, greedy 5–15 minutes, after its survey and first digs.
- **Random matches greedy.** Their headlines are within 1.9% of each other in every scenario, and random is marginally ahead in some. Greedy, as specified, gives each ready task to the cheapest free robot, but robots free up one at a time as their commands end, so a task rarely has more than one candidate and both policies give it to the same robot. Choosing among robots matters only when several are free at once: at the start, after charging, and after rescues.
- **First come, first served falls far behind** (0.385, 0.419 and 0.362). The dig tasks for the whole pad are created first, after the survey, so the excavators dig every cell before grading any, and the compactor-sinterers wait for graded cells.
- **Where ArcadiaOS does not win.**
  - *Resilience on Medium and Large.* Greedy keeps slightly more of its own nominal headline under stress (0.980 and 0.990, against ArcadiaOS's 0.976 and 0.987). A fault costs about the same time whoever meets it: under `cascading_faults` ArcadiaOS's Medium build takes 640 s longer than nominal and greedy's 574 s, but that is a larger share of ArcadiaOS's shorter build. Its stressed headlines are still the highest in every case. Its suite resilience is the highest overall because of Small, where it keeps 0.963 of its nominal headline against greedy's 0.932, and meets about half as many faults in the dust storm (0.48 a run against 0.84).
  - *Energy per pad on Small and Medium.* ArcadiaOS draws 35.9 and 35.2 kWh per pad against greedy's 34.7 and 34.6. The baselines cast dig spoil straight onto low cells anywhere on the pad, which the protocol allows; ArcadiaOS casts at most 3 m and hauls the rest. On Large it draws less (35.9 against 36.6).
  - *Messages to Earth.* ArcadiaOS sends about 1.5 messages a run (a status report, and notifies of replans and tows), the baselines 0.02–0.06 (notifies of tows). None needs an answer.
- **The conjunction costs nothing.** No coordinator waits on Earth during these builds: the baselines never ask, and nothing in ArcadiaOS's default policy needs an answer. The blackout covers 3.2 h of ArcadiaOS's 3.7-hour Medium build and 1.4 h of its 2.0-hour Large build. A coordinator that waited for approvals would pay for it.
- **The power variants score as nominal.** No build drains the bank, which starts 60% full: the lowest storage in any run was 43%, in `solar_only`. `solar_only` and `nuclear_heavy` would matter for longer builds; they are not stress variants.
- **The builds are in the dark.** `t = 0` is local midnight and sunrise is at sol phase 0.25. Every Medium and Large build by ArcadiaOS, greedy and random ends before sunrise (the slowest takes 0.245 sols); only first come, first served's slowest Medium builds see the sun. So on Medium and Large the dust storm brings sensor faults and wear, not lost sunlight. Small builds take most of a sol and work through the stormy day.
- **Quality is the same for all.** Every pad met the ±5 cm tolerance and the slope limit, with an RMS error of about 11 mm: grading leaves a cell within millimetres of the target and compaction then settles it about a centimetre.

## The coordinators

- **ArcadiaOS 1.0** ([repository](https://github.com/Gabe-Soler/arcadiaos)): capacity planning by linear program, a sequential auction for dispatch, predictive maintenance, energy-aware charging, the authority layer and coverage-aware planning. Its `docs/architecture.md` describes it.
- **The baselines** ([`src/baselines/`](../../src/baselines/), binaries `baseline-random`, `baseline-greedy` and `baseline-fcfs`) are honest protocol clients: they use only `arcadia_protocol`, and `tests/baselines.rs` checks this. They share a task decomposition (each pad cell through survey, dig, fill if low, grade, compact, sinter and verify; dig spoil cast onto low cells, then the berm, then stockpiles; borrow pits for the berm), an execution loop, and the safety behaviours: charge below 20% of the battery, field repair or tow-and-service for faults, retry after `out_of_coverage`, a `notify` for every level-2 command and nothing at level 3 or 4. They differ only in allocation:
  - **random:** ready tasks in priority order; each goes to a capable free robot drawn from a generator seeded with the run's seed.
  - **greedy:** ready tasks in priority order; each goes to the capable free robot with the lowest estimated cost, travel time plus work time.
  - **first come, first served (FCFS):** robots in the order they became free; each takes the oldest ready task it can do.

  Priority order is rescans and surveys, then verification, then the pad pipeline from its downstream end (sinter, compact, grade, fill, dig), then the berm; within a class, cells in sweep order. A robot only takes a task its battery covers, with the trip to a station after it.

### ArcadiaOS tuning

Benchmarking against the baselines led to one change in ArcadiaOS's allocation before these results were taken. Its capacity plan splits each capability's work across the robots that can do it, and the split is a vertex of a linear program, so among identical compactor-sinterers it gave all the compaction to one of them. The other sinterers chased that robot's moving front: on Large seed 1 they drove 3.8 cells on average before each sinter. Now a robot may take the pad step just before one of its roles without the help penalty, so a sinterer compacts the next cell and sinters it close by (1.4 cells on the same seed). Against this simulator, on seeds 1–20, Large's nominal headline rose from 0.651 to 0.706 and Medium's from 0.736 to 0.750; Small, with one compactor-sinterer, was unchanged. No run failed before or after: ArcadiaOS finished every seed of every scenario.

## Full scorecards

Every field of every scorecard is in [`scorecards/`](scorecards/) and [`results.json`](results.json). These tables show the main ones; quantities are per run unless stated, and sols, speed and pad error are over completed runs (all of them here).

### Small

#### `landing-pad-small-nominal`

Reference build time 0.451 sols. Conditions per run: storm 0.1 h, blackout 0.0 h, unplanned outage 0.6 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.559 | 100% | 0.806 (0.788–0.896) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.18 | 0.14 | 0.00 | 0.00 | 1.52 | 0.00 |
| greedy | 0.501 | 100% | 0.900 (0.880–0.970) | 100.0% | 11.4 mm | 34.7 | 60.0% | 0.14 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| random | 0.499 | 100% | 0.904 (0.886–0.989) | 100.0% | 11.4 mm | 34.7 | 60.0% | 0.12 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| FCFS | 0.385 | 100% | 1.174 (1.080–1.472) | 100.0% | 11.3 mm | 38.8 | 60.0% | 0.24 | 0.17 | 0.00 | 0.00 | 0.06 | 0.00 |

#### `landing-pad-small-dust_storm`

Reference build time 0.451 sols. Conditions per run: storm 20.2 h, blackout 0.0 h, unplanned outage 0.6 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.543 | 100% | 0.830 (0.812–0.909) | 100.0% | 11.3 mm | 36.1 | 60.0% | 0.48 | 0.28 | 0.00 | 0.00 | 2.10 | 0.00 |
| greedy | 0.456 | 100% | 0.989 (0.950–1.097) | 100.0% | 11.4 mm | 36.1 | 60.0% | 0.84 | 0.57 | 0.00 | 0.00 | 0.20 | 0.00 |
| random | 0.453 | 100% | 0.995 (0.956–1.097) | 100.0% | 11.4 mm | 36.1 | 60.0% | 0.90 | 0.65 | 0.00 | 0.00 | 0.24 | 0.00 |
| FCFS | 0.349 | 100% | 1.304 (1.165–2.149) | 100.0% | 11.4 mm | 40.5 | 60.0% | 1.72 | 1.21 | 0.00 | 0.00 | 0.24 | 0.00 |

#### `landing-pad-small-conjunction`

Reference build time 0.451 sols. Conditions per run: storm 0.1 h, blackout 19.3 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.560 | 100% | 0.805 (0.794–0.889) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.14 | 0.10 | 0.00 | 0.00 | 1.44 | 0.00 |
| greedy | 0.501 | 100% | 0.901 (0.882–0.974) | 100.0% | 11.4 mm | 34.7 | 60.0% | 0.18 | 0.11 | 0.00 | 0.00 | 0.04 | 0.00 |
| random | 0.496 | 100% | 0.909 (0.884–1.150) | 100.0% | 11.3 mm | 34.9 | 60.0% | 0.16 | 0.11 | 0.00 | 0.00 | 0.04 | 0.00 |
| FCFS | 0.385 | 100% | 1.175 (1.082–1.477) | 100.0% | 11.3 mm | 38.8 | 60.0% | 0.32 | 0.20 | 0.00 | 0.00 | 0.08 | 0.00 |

#### `landing-pad-small-cascading_faults`

Reference build time 0.451 sols. Conditions per run: storm 0.1 h, blackout 0.0 h, unplanned outage 0.6 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.514 | 100% | 0.877 (0.854–0.949) | 100.0% | 11.4 mm | 37.4 | 60.0% | 1.02 | 0.65 | 0.00 | 0.00 | 3.54 | 0.00 |
| greedy | 0.444 | 100% | 1.016 (0.954–1.088) | 100.0% | 11.4 mm | 37.9 | 60.0% | 1.84 | 1.23 | 0.00 | 0.00 | 0.48 | 0.00 |
| random | 0.443 | 100% | 1.018 (0.957–1.142) | 100.0% | 11.4 mm | 38.0 | 60.0% | 1.88 | 1.13 | 0.00 | 0.00 | 0.44 | 0.00 |
| FCFS | 0.325 | 100% | 1.403 (1.218–2.205) | 100.0% | 11.3 mm | 42.5 | 60.0% | 2.68 | 2.36 | 0.00 | 0.00 | 0.66 | 0.00 |

#### `landing-pad-small-solar_only`

Reference build time 0.451 sols. Conditions per run: storm 0.1 h, blackout 0.0 h, unplanned outage 0.6 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.560 | 100% | 0.805 (0.792–0.883) | 100.0% | 11.3 mm | 35.9 | 44.1% | 0.14 | 0.10 | 0.00 | 0.00 | 1.50 | 0.00 |
| greedy | 0.501 | 100% | 0.900 (0.880–0.970) | 100.0% | 11.4 mm | 34.7 | 43.2% | 0.14 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| random | 0.499 | 100% | 0.904 (0.886–0.989) | 100.0% | 11.4 mm | 34.7 | 43.2% | 0.12 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| FCFS | 0.385 | 100% | 1.174 (1.080–1.472) | 100.0% | 11.3 mm | 38.8 | 48.4% | 0.24 | 0.17 | 0.00 | 0.00 | 0.06 | 0.00 |

#### `landing-pad-small-nuclear_heavy`

Reference build time 0.451 sols. Conditions per run: storm 0.1 h, blackout 0.0 h, unplanned outage 0.6 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.559 | 100% | 0.806 (0.788–0.896) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.18 | 0.14 | 0.00 | 0.00 | 1.52 | 0.00 |
| greedy | 0.501 | 100% | 0.900 (0.880–0.970) | 100.0% | 11.4 mm | 34.7 | 60.0% | 0.14 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| random | 0.499 | 100% | 0.904 (0.886–0.989) | 100.0% | 11.4 mm | 34.7 | 60.0% | 0.12 | 0.10 | 0.00 | 0.00 | 0.04 | 0.00 |
| FCFS | 0.385 | 100% | 1.174 (1.080–1.472) | 100.0% | 11.3 mm | 38.8 | 60.0% | 0.24 | 0.17 | 0.00 | 0.00 | 0.06 | 0.00 |

### Medium

#### `landing-pad-medium-nominal`

Reference build time 0.113 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.1 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.753 | 100% | 0.150 (0.144–0.178) | 100.0% | 11.4 mm | 35.2 | 60.0% | 0.28 | 0.14 | 0.00 | 0.00 | 1.50 | 0.00 |
| greedy | 0.681 | 100% | 0.167 (0.155–0.225) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.26 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.677 | 100% | 0.168 (0.156–0.245) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.419 | 100% | 0.271 (0.248–0.418) | 100.0% | 11.4 mm | 39.1 | 60.0% | 0.30 | 0.14 | 0.00 | 0.00 | 0.04 | 0.00 |

#### `landing-pad-medium-dust_storm`

Reference build time 0.113 sols. Conditions per run: storm 3.5 h, blackout 0.0 h, unplanned outage 0.1 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.732 | 100% | 0.154 (0.147–0.172) | 100.0% | 11.3 mm | 35.5 | 60.0% | 0.42 | 0.24 | 0.00 | 0.00 | 1.78 | 0.00 |
| greedy | 0.665 | 100% | 0.171 (0.160–0.237) | 100.0% | 11.3 mm | 34.9 | 60.0% | 0.46 | 0.07 | 0.00 | 0.00 | 0.04 | 0.00 |
| random | 0.653 | 100% | 0.175 (0.159–0.242) | 100.0% | 11.4 mm | 35.1 | 60.0% | 0.56 | 0.24 | 0.00 | 0.00 | 0.14 | 0.00 |
| FCFS | 0.400 | 100% | 0.287 (0.250–0.424) | 100.0% | 11.4 mm | 39.6 | 60.0% | 1.04 | 0.40 | 0.00 | 0.00 | 0.16 | 0.00 |

#### `landing-pad-medium-conjunction`

Reference build time 0.113 sols. Conditions per run: storm 0.0 h, blackout 3.2 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.753 | 100% | 0.150 (0.144–0.178) | 100.0% | 11.4 mm | 35.2 | 60.0% | 0.28 | 0.14 | 0.00 | 0.00 | 1.50 | 0.00 |
| greedy | 0.681 | 100% | 0.167 (0.155–0.225) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.26 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.677 | 100% | 0.168 (0.156–0.245) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.416 | 100% | 0.275 (0.248–0.420) | 100.0% | 11.4 mm | 39.1 | 60.0% | 0.30 | 0.14 | 0.00 | 0.00 | 0.04 | 0.00 |

#### `landing-pad-medium-cascading_faults`

Reference build time 0.113 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.1 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.719 | 100% | 0.157 (0.148–0.175) | 100.0% | 11.3 mm | 36.2 | 60.0% | 0.88 | 0.48 | 0.00 | 0.00 | 2.62 | 0.00 |
| greedy | 0.655 | 100% | 0.173 (0.161–0.234) | 100.0% | 11.3 mm | 35.6 | 60.0% | 0.94 | 0.33 | 0.00 | 0.00 | 0.08 | 0.00 |
| random | 0.647 | 100% | 0.176 (0.161–0.238) | 100.0% | 11.4 mm | 35.6 | 60.0% | 0.92 | 0.43 | 0.00 | 0.00 | 0.22 | 0.00 |
| FCFS | 0.375 | 100% | 0.309 (0.254–0.434) | 100.0% | 11.3 mm | 40.6 | 60.0% | 1.20 | 0.79 | 0.00 | 0.00 | 0.28 | 0.00 |

#### `landing-pad-medium-solar_only`

Reference build time 0.113 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.1 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.753 | 100% | 0.150 (0.144–0.178) | 100.0% | 11.4 mm | 35.2 | 54.1% | 0.28 | 0.14 | 0.00 | 0.00 | 1.50 | 0.00 |
| greedy | 0.681 | 100% | 0.167 (0.155–0.225) | 100.0% | 11.3 mm | 34.6 | 52.6% | 0.26 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.677 | 100% | 0.168 (0.156–0.245) | 100.0% | 11.3 mm | 34.6 | 52.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.419 | 100% | 0.271 (0.248–0.418) | 100.0% | 11.4 mm | 39.1 | 49.8% | 0.30 | 0.14 | 0.00 | 0.00 | 0.04 | 0.00 |

#### `landing-pad-medium-nuclear_heavy`

Reference build time 0.113 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.1 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.753 | 100% | 0.150 (0.144–0.178) | 100.0% | 11.4 mm | 35.2 | 60.0% | 0.28 | 0.14 | 0.00 | 0.00 | 1.50 | 0.00 |
| greedy | 0.681 | 100% | 0.167 (0.155–0.225) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.26 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.677 | 100% | 0.168 (0.156–0.245) | 100.0% | 11.3 mm | 34.6 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.419 | 100% | 0.271 (0.248–0.418) | 100.0% | 11.4 mm | 39.1 | 60.0% | 0.30 | 0.14 | 0.00 | 0.00 | 0.04 | 0.00 |

### Large

#### `landing-pad-large-nominal`

Reference build time 0.056 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.710 | 100% | 0.079 (0.077–0.087) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.24 | 0.07 | 0.00 | 0.00 | 1.54 | 0.00 |
| greedy | 0.585 | 100% | 0.097 (0.091–0.149) | 100.0% | 11.4 mm | 36.6 | 60.0% | 0.20 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.587 | 100% | 0.097 (0.091–0.124) | 100.0% | 11.4 mm | 36.5 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.362 | 100% | 0.156 (0.148–0.172) | 100.0% | 11.3 mm | 41.3 | 60.0% | 0.32 | 0.18 | 0.00 | 0.00 | 0.10 | 0.00 |

#### `landing-pad-large-dust_storm`

Reference build time 0.056 sols. Conditions per run: storm 1.7 h, blackout 0.0 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.701 | 100% | 0.080 (0.077–0.089) | 100.0% | 11.3 mm | 36.0 | 60.0% | 0.38 | 0.10 | 0.00 | 0.00 | 1.58 | 0.00 |
| greedy | 0.580 | 100% | 0.098 (0.092–0.128) | 100.0% | 11.4 mm | 36.7 | 60.0% | 0.60 | 0.19 | 0.00 | 0.00 | 0.06 | 0.00 |
| random | 0.581 | 100% | 0.098 (0.092–0.126) | 100.0% | 11.4 mm | 36.7 | 60.0% | 0.70 | 0.19 | 0.00 | 0.00 | 0.08 | 0.00 |
| FCFS | 0.356 | 100% | 0.158 (0.149–0.174) | 100.0% | 11.3 mm | 41.7 | 60.0% | 1.34 | 0.40 | 0.00 | 0.00 | 0.18 | 0.00 |

#### `landing-pad-large-conjunction`

Reference build time 0.056 sols. Conditions per run: storm 0.0 h, blackout 1.4 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.710 | 100% | 0.079 (0.077–0.087) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.24 | 0.07 | 0.00 | 0.00 | 1.54 | 0.00 |
| greedy | 0.585 | 100% | 0.097 (0.091–0.149) | 100.0% | 11.4 mm | 36.6 | 60.0% | 0.20 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.587 | 100% | 0.097 (0.091–0.124) | 100.0% | 11.4 mm | 36.5 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.362 | 100% | 0.156 (0.148–0.172) | 100.0% | 11.3 mm | 41.3 | 60.0% | 0.32 | 0.18 | 0.00 | 0.00 | 0.10 | 0.00 |

#### `landing-pad-large-cascading_faults`

Reference build time 0.056 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.692 | 100% | 0.082 (0.078–0.089) | 100.0% | 11.3 mm | 36.5 | 60.0% | 0.96 | 0.33 | 0.00 | 0.04 | 2.88 | 0.00 |
| greedy | 0.573 | 100% | 0.099 (0.092–0.132) | 100.0% | 11.3 mm | 37.1 | 60.0% | 0.92 | 0.39 | 0.00 | 0.00 | 0.12 | 0.00 |
| random | 0.572 | 100% | 0.099 (0.092–0.127) | 100.0% | 11.4 mm | 37.1 | 60.0% | 0.92 | 0.48 | 0.00 | 0.00 | 0.20 | 0.00 |
| FCFS | 0.351 | 100% | 0.161 (0.149–0.183) | 100.0% | 11.3 mm | 42.3 | 60.0% | 1.18 | 0.60 | 0.00 | 0.00 | 0.22 | 0.00 |

#### `landing-pad-large-solar_only`

Reference build time 0.056 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.710 | 100% | 0.079 (0.077–0.087) | 100.0% | 11.3 mm | 35.9 | 57.1% | 0.24 | 0.07 | 0.00 | 0.00 | 1.54 | 0.00 |
| greedy | 0.585 | 100% | 0.097 (0.091–0.149) | 100.0% | 11.4 mm | 36.6 | 55.1% | 0.20 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.587 | 100% | 0.097 (0.091–0.124) | 100.0% | 11.4 mm | 36.5 | 55.9% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.362 | 100% | 0.156 (0.148–0.172) | 100.0% | 11.3 mm | 41.3 | 54.4% | 0.32 | 0.18 | 0.00 | 0.00 | 0.10 | 0.00 |

#### `landing-pad-large-nuclear_heavy`

Reference build time 0.056 sols. Conditions per run: storm 0.0 h, blackout 0.0 h, unplanned outage 0.0 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | kWh per pad | Lowest storage | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.710 | 100% | 0.079 (0.077–0.087) | 100.0% | 11.3 mm | 35.9 | 60.0% | 0.24 | 0.07 | 0.00 | 0.00 | 1.54 | 0.00 |
| greedy | 0.585 | 100% | 0.097 (0.091–0.149) | 100.0% | 11.4 mm | 36.6 | 60.0% | 0.20 | 0.07 | 0.00 | 0.00 | 0.02 | 0.00 |
| random | 0.587 | 100% | 0.097 (0.091–0.124) | 100.0% | 11.4 mm | 36.5 | 60.0% | 0.20 | 0.13 | 0.00 | 0.00 | 0.06 | 0.00 |
| FCFS | 0.362 | 100% | 0.156 (0.148–0.172) | 100.0% | 11.3 mm | 41.3 | 60.0% | 0.32 | 0.18 | 0.00 | 0.00 | 0.10 | 0.00 |

## Reproduce

The results come from:

| Software | Version | Release |
|---|---|---|
| ArcadiaBench (`arcadiabench` and the baselines) | 1.0.0 | tag `v1.0.0` |
| ArcadiaOS (`arcadiaos`) | 1.0.0 | tag `v1.0.0` |
| arcadia-protocol | 1.0.0 | tag `v1.0.0` |
| Rust | rustc 1.99.1, release builds | |

They were run on macOS 26.5.2 on an Apple M4 Pro (arm64, 14 CPUs). Every run is deterministic, and what a batch writes does not depend on `--jobs`, so any machine reproduces these files byte for byte.

From the root of ArcadiaBench, with ArcadiaOS checked out beside it, build both and run [`scripts/results.sh`](../../scripts/results.sh):

```sh
cargo build --release
(cd ../arcadiaos && cargo build --release)
scripts/results.sh
```

With its runs in a temporary directory `$RUNS`, the script does this:

```sh
for c in arcadiaos baseline-greedy baseline-random baseline-fcfs; do
  if [ "$c" = arcadiaos ]; then coord=$ARCADIAOS; else coord=target/release/$c; fi
  target/release/arcadiabench suite --seeds 1-50 --out "$RUNS/$c" -- "$coord"
  mkdir -p results/v1.0/scorecards/$c
  for d in "$RUNS/$c"/*/; do
    cp "$d/scorecard.json" "results/v1.0/scorecards/$c/$(basename "$d").json"
  done
done
target/release/arcadiabench score --json "$RUNS"/*/*/metrics.jsonl > results/v1.0/results.json
```

`ARCADIAOS` names the ArcadiaOS binary and defaults to `../arcadiaos/target/release/arcadiaos`. Pass the script a directory to write the files there instead of `results/v1.0`, for example to compare a fresh run with these. Each `suite` does 900 runs, one per CPU at a time by default (`--jobs`); on the machine above each took under half a minute. Keep `--jobs` at or below the number of CPUs, because the per-decision timeout is wall-clock. `score` reads the run records each scenario's `metrics.jsonl` holds and rebuilds every scorecard and the resilience of every coordinator, identical to what the suites wrote. The demo replays in [`replays/`](replays/) come from [`scripts/demos.sh`](../../scripts/demos.sh), and [`docs/demos.md`](../../docs/demos.md) shows its commands.

## Files

- [`results.json`](results.json): every scorecard and each coordinator's resilience, as a report (`schemas/scorecard.schema.json#/$defs/Report`).
- [`scorecards/<coordinator>/<preset>-<variant>.json`](scorecards/): one scorecard per coordinator and scenario (the schema's root).
- [`replays/`](replays/): ArcadiaOS's four demo runs as gzipped event logs (protocol 1.0.0 `event_log.schema.json`), for the replay viewer. [`docs/demos.md`](../../docs/demos.md) describes them.

No other raw logs are kept. `tests/results.rs` checks that these files match their schemas, that the scorecards agree with `results.json`, and that the replays are valid logs within the size limit.
