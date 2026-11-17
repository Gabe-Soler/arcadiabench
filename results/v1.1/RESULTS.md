# ArcadiaBench v1.1 results

The benchmark results of ArcadiaBench 1.1: ArcadiaOS 1.1 and the three baseline coordinators that ship with ArcadiaBench, on every preset and variant of catalog 1.1, over seeds 1–50. That is 18 scenarios × 50 seeds × 4 coordinators, 3,600 runs in all, of which 3,599 completed the pad. The fleet design track adds 600 runs: the crew ArcadiaOS designs, run by ArcadiaOS and by the greedy baseline.

> **ArcadiaOS finishes the pad 11% faster than greedy. On the nominal build greedy never asks Earth for anything, and ArcadiaOS asks about once a run on Large.**
>
> That is the outline's headline, "X% faster than greedy, with Y fewer Earth interventions", computed on the nominal build and averaged over the three presets: X = 11%. Y is not in ArcadiaOS's favour on this catalog. See [the headline sentence](#the-headline-sentence).

Catalog 1.1 is the default catalog of ArcadiaBench 1.1. A build on it takes weeks where catalog 1.0's took hours, so these results are not comparable with [v1.0's](../v1.0/RESULTS.md), which stay as published.

Contents: [headline](#headline) · [every scenario](#every-scenario) · [resilience](#resilience) · [the fleet design track](#the-fleet-design-track) · [what the results show](#what-the-results-show) · [what the catalog does not do](#what-the-catalog-does-not-do) · [the coordinators](#the-coordinators) · [full scorecards](#full-scorecards) · [reproduce](#reproduce) · [files](#files)

## Headline

The headline metric is **success rate × mean speed** on the nominal variant, where speed is the reference build time over the actual one.

| Coordinator | Small | Medium | Large |
|---|---|---|---|
| ArcadiaOS | **0.764** | **0.764** | **0.692** |
| greedy | 0.690 | 0.683 | 0.627 |
| random | 0.673 | 0.678 | 0.628 |
| FCFS | 0.660 | 0.659 | 0.606 |

Build time on the nominal variant, sols: mean (fastest–slowest) over the 50 seeds.

| Coordinator | Small | Medium | Large |
|---|---|---|---|
| ArcadiaOS | 49.1 (47.7–50.2) | 24.6 (23.9–25.7) | 16.3 (16.0–17.3) |
| greedy | 54.7 (50.3–69.5) | 27.7 (24.6–35.0) | 18.1 (16.2–20.9) |
| random | 56.2 (50.1–74.4) | 28.0 (24.6–38.0) | 18.0 (16.2–21.8) |
| FCFS | 57.3 (50.2–79.8) | 28.6 (25.5–35.4) | 18.7 (16.5–24.1) |

### The headline sentence

The project outline asks for one sentence: "X% faster than greedy, with Y fewer Earth interventions". On the nominal build:

| Preset | ArcadiaOS headline | Greedy headline | Faster | Approval requests per run (ArcadiaOS, greedy) |
|---|---|---|---|---|
| small | 0.764 | 0.690 | 10.8% | 0.00, 0.00 |
| medium | 0.764 | 0.683 | 11.9% | 0.42, 0.00 |
| large | 0.692 | 0.627 | 10.4% | 1.02, 0.00 |

Averaged over the presets, ArcadiaOS is 11% faster. It asks Earth more often than greedy, not less: before it charges robots from the last quarter of the site bank it asks for approval, as the policy requires of `commit_energy_reserve`, which is level 4. The baselines never ask. The simulator cannot see that action, so it counts no violation against them.

## Every scenario

Headline per scenario. Every coordinator completed all 50 seeds unless a count is given.

| Scenario | ArcadiaOS | greedy | random | FCFS |
|---|---|---|---|---|
| `small` / `nominal` | **0.764** | 0.690 | 0.673 | 0.660 |
| `small` / `dust_storm` | **0.760** | 0.667 | 0.662 | 0.644 |
| `small` / `conjunction` | **0.762** | 0.688 | 0.680 | 0.659 |
| `small` / `cascading_faults` | **0.722** | 0.630 (49 of 50) | 0.626 | 0.613 |
| `small` / `solar_only` | **0.761** | 0.676 | 0.673 | 0.660 |
| `small` / `nuclear_heavy` | **0.764** | 0.690 | 0.673 | 0.660 |
| `medium` / `nominal` | **0.764** | 0.683 | 0.678 | 0.659 |
| `medium` / `dust_storm` | **0.761** | 0.663 | 0.663 | 0.638 |
| `medium` / `conjunction` | **0.764** | 0.686 | 0.679 | 0.653 |
| `medium` / `cascading_faults` | **0.726** | 0.654 | 0.655 | 0.625 |
| `medium` / `solar_only` | **0.708** | 0.649 | 0.634 | 0.618 |
| `medium` / `nuclear_heavy` | **0.766** | 0.682 | 0.677 | 0.659 |
| `large` / `nominal` | **0.692** | 0.627 | 0.628 | 0.606 |
| `large` / `dust_storm` | **0.639** | 0.570 | 0.569 | 0.553 |
| `large` / `conjunction` | **0.690** | 0.621 | 0.627 | 0.607 |
| `large` / `cascading_faults` | **0.664** | 0.599 | 0.591 | 0.587 |
| `large` / `solar_only` | **0.496** | 0.450 | 0.451 | 0.438 |
| `large` / `nuclear_heavy` | **0.762** | 0.676 | 0.674 | 0.656 |

## Resilience

Resilience is, per preset, the mean over `dust_storm`, `conjunction` and `cascading_faults` of each one's headline over the nominal headline, capped at 1. The suite figure is the mean over the presets.

| Coordinator | Small | Medium | Large | Suite |
|---|---|---|---|---|
| ArcadiaOS | 0.979 | 0.982 | 0.960 | **0.974** |
| greedy | 0.959 | 0.976 | 0.952 | 0.962 |
| random | 0.971 | 0.981 | 0.948 | 0.967 |
| FCFS | 0.968 | 0.969 | 0.960 | 0.966 |

Each stress variant's headline over the nominal headline:

| Coordinator | Preset | `dust_storm` | `conjunction` | `cascading_faults` |
|---|---|---|---|---|
| ArcadiaOS | small | 0.995 | 0.997 | 0.945 |
| ArcadiaOS | medium | 0.996 | 1.000 | 0.950 |
| ArcadiaOS | large | 0.923 | 0.998 | 0.959 |
| greedy | small | 0.967 | 0.997 | 0.913 |
| greedy | medium | 0.971 | 1.000 | 0.958 |
| greedy | large | 0.909 | 0.991 | 0.956 |
| random | small | 0.983 | 1.000 | 0.930 |
| random | medium | 0.978 | 1.000 | 0.966 |
| random | large | 0.906 | 0.998 | 0.941 |
| FCFS | small | 0.976 | 0.999 | 0.930 |
| FCFS | medium | 0.967 | 0.991 | 0.948 |
| FCFS | large | 0.912 | 1.000 | 0.969 |

## The fleet design track

On this track the coordinator brings its own crew: a count for each of the catalog's four designs, of 4 to 20 robots and no more than 10 t in all. Every crew works from the Medium outpost, and speed is measured against one reference for every crew: the time the pad would take with as many sinterers as the budget carries (10.2 sols).

ArcadiaOS designs its crew with `arcadiaos design-crew`, from what `arcadiabench scenario --goal` prints: **2 scouts, 1 excavator, 1 hauler and 10 compactor-sinterers**, 9,900 kg ([`fleet/crew.json`](fleet/crew.json)). The baselines have no designer, so there is one entry. The greedy baseline with ArcadiaOS's crew is shown beside it, to separate the crew from how it is used.

| Variant | ArcadiaOS: headline | sols | greedy, same crew: headline | sols |
|---|---|---|---|---|
| `nominal` | **0.634** | 16.1 (15.9–16.9) | 0.571 | 18.1 (16.2–22.6) |
| `dust_storm` | **0.586** | 17.5 (16.9–18.4) | 0.529 | 19.5 (17.6–23.0) |
| `conjunction` | **0.632** | 16.2 (15.9–17.4) | 0.575 | 17.9 (16.1–21.9) |
| `cascading_faults` | **0.608** | 16.9 (16.4–18.3) | 0.553 | 18.8 (16.8–26.3) |
| `solar_only` | **0.454** | 22.6 (21.6–25.6) | 0.426 | 24.2 (21.9–29.3) |
| `nuclear_heavy` | **0.686** | 14.9 (14.5–15.5) | 0.622 | 16.6 (14.7–21.2) |
| mean | **0.600** | | 0.546 | |

All 600 runs completed. Resilience with this crew is 0.959 for ArcadiaOS and 0.965 for greedy.

- **The crew is worth more than the Medium preset's.** On the same outpost ArcadiaOS builds in 16.1 sols with its own crew and in 24.6 with the Medium preset's (2 scouts, 2 excavators, 2 haulers, 6 compactor-sinterers, 7,020 kg). The two headlines are not comparable: the presets are scored against their own crew's reference.
- **Why ten sinterers.** The plant has about 38 kW to spare over a sol, which feeds nine to ten compactor-sinterers. An eleventh would wait for power, so the designer spends the rest of the mass on the robots the build cannot do without: two scouts, an excavator, and a hauler to charge a robot in the field. ArcadiaOS's `docs/architecture.md` describes the model.
- **Where an eleventh would pay.** The crew is designed for the nominal plant. Under `nuclear_heavy` power is plentiful and the build is 14.9 sols; a crew for that plant would carry more sinterers.

## What the results show

- **ArcadiaOS has the best headline in every scenario**, by 9 to 15% over greedy. It completed all 900 of its runs with no policy violation and no robot lost.
- **Where the time goes.** Sintering 400 cells at 37 kWh each takes 14.8 MWh and seven hours a cell. That is nearly all of a build.
  - *Small and Medium* are bound by the compactor-sinterers: three finish in about 49 sols and six in about 25.
  - *Large* is bound by the plant. Ten sinterers want more than the outpost generates, and the site bank is empty for 182 of the 402 hours of ArcadiaOS's nominal build.
  - *Energy per pad.* ArcadiaOS uses 15.3, 15.3 and 15.4 MWh on Small, Medium and Large; greedy 16.7, 16.9 and 16.7. Neither sinters a cell twice in the runs looked at. The difference is wear, which makes every step cost more (in one Medium run a cell cost greedy 38.0 kWh and 7.2 hours to sinter, and ArcadiaOS, which repairs before wear builds up, 37.5 kWh and 6.9 hours), and the driving, idling and heating of a longer build.
- **The power variants matter now.** With solar power only, ArcadiaOS keeps 100%, 93% and 72% of its nominal headline on Small, Medium and Large. With the larger reactor of `nuclear_heavy` its Large headline rises from 0.692 to 0.762, and Small and Medium, which the plant does not limit, are unchanged.
- **The dust storm costs most where power is short.** On Large it takes 8% off ArcadiaOS's headline and 9% off greedy's; on Small and Medium, under 1% and about 3%.
- **Earth.** ArcadiaOS sends 75, 54 and 47 messages a run on Small, Medium and Large: a status report a sol, and notifies. It asks for approval 0.00, 0.42 and 1.02 times a run, mostly to charge from the bank's reserve, and an answer takes 11 hours on Large: Earth is reachable only in passes and takes about four hours to answer. It cannibalized no robot in any run.
- **Quality.** Every completed pad met the ±5 cm tolerance and the 2° slope limit. The RMS error is 7 to 8 mm, from the grading and settlement errors the catalog adds.

## What the catalog does not do

Catalog 1.1 was designed to tell coordinators apart where catalog 1.0 did not. It does in part: builds take weeks, the power variants differ from nominal, and a coordinator that plans for catalog 1.0's numbers does not finish (ArcadiaOS 1.0's rules, ported to protocol 1.1, time out on Small). Three of its design criteria are not met, and the catalog is released as it is, with them stated. They are for the next catalog.

- **Stress does not make a coordinator fail.** 1 of 3,600 runs did not complete: greedy on `small` / `cascading_faults`, where the crew could no longer work (`fleet_lost`). No run ran out of time, and no robot was stripped for parts.
- **The conjunction costs almost nothing.** Through the two-week blackout every coordinator keeps 99.1% to 100.0% of its nominal headline. Nothing a build needs waits on Earth: the baselines never ask, and what ArcadiaOS asks for it can do without.
- **Random matches greedy.** Greedy's headline is on average 0.8% above random's, and between 0.9% below and 2.5% above. First come, first served is 2% to 5% behind greedy, where on catalog 1.0 it was 23 to 43% behind. When a cell takes seven hours to sinter, which robot takes which cell matters little; the three baselines share everything else.

## The coordinators

- **ArcadiaOS 1.1** ([repository](https://github.com/Gabe-Soler/arcadiaos)). What it adds to 1.0 for this catalog: it grades for the settlement the simulator states, measures a compacted cell with averaged scans before it sinters it, reads wear from what each robot shows, holds its plan to what the plant can deliver, prices a robot by the build time lost without it, and decides when something happens. Its `docs/architecture.md` describes it.
- **The baselines** ([`src/baselines/`](../../src/baselines/), binaries `baseline-random`, `baseline-greedy` and `baseline-fcfs`) are honest protocol clients: they use only `arcadia_protocol`, and `tests/baselines.rs` checks this. They share a task decomposition and differ only in who takes which task; [v1.0's results](../v1.0/RESULTS.md#the-coordinators) describe the three rules. Since 1.0 they grade for the stated settlement and wake hourly. They do not measure before sintering, do no maintenance beyond repairing what fails, and do not ask Earth.

## Full scorecards

Every field of every scorecard is in [`scorecards/`](scorecards/) and [`results.json`](results.json). These tables show the main ones; quantities are per run unless stated, and sols, speed and pad error are over completed runs.

### Small

#### `landing-pad-1.1-small-nominal`

Reference build time 37.5 sols. Conditions per run: storm 126 h, blackout 0 h, unplanned outage 9 h, between passes 1045 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.764 | 100% | 49.1 (47.7–50.2) | 100.0% | 7.7 mm | 15.3 | 0 h | 23.8 | 9.7 | 0.00 | 0.00 | 74.9 | 0.00 |
| greedy | 0.690 | 100% | 54.7 (50.3–69.5) | 100.0% | 7.4 mm | 16.7 | 0 h | 27.1 | 27.9 | 0.00 | 0.00 | 4.1 | 0.00 |
| random | 0.673 | 100% | 56.2 (50.1–74.4) | 100.0% | 7.3 mm | 17.1 | 0 h | 28.7 | 29.5 | 0.00 | 0.00 | 3.5 | 0.00 |
| FCFS | 0.660 | 100% | 57.3 (50.2–79.8) | 100.0% | 7.4 mm | 16.9 | 0 h | 28.2 | 25.5 | 0.00 | 0.00 | 3.5 | 0.00 |

#### `landing-pad-1.1-small-dust_storm`

Reference build time 37.5 sols. Conditions per run: storm 890 h, blackout 0 h, unplanned outage 9 h, between passes 1050 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.760 | 100% | 49.4 (47.9–52.1) | 100.0% | 7.7 mm | 15.3 | 0 h | 24.8 | 9.7 | 0.00 | 0.00 | 73.7 | 0.00 |
| greedy | 0.667 | 100% | 56.5 (51.4–66.8) | 100.0% | 7.4 mm | 17.0 | 0 h | 40.1 | 40.6 | 0.00 | 0.00 | 5.8 | 0.00 |
| random | 0.662 | 100% | 57.1 (51.2–68.9) | 100.0% | 7.3 mm | 17.1 | 0 h | 40.8 | 39.2 | 0.00 | 0.00 | 5.3 | 0.00 |
| FCFS | 0.644 | 100% | 58.7 (53.0–76.0) | 100.0% | 7.4 mm | 17.0 | 0 h | 40.4 | 39.9 | 0.00 | 0.00 | 5.5 | 0.00 |

#### `landing-pad-1.1-small-conjunction`

Reference build time 37.5 sols. Conditions per run: storm 126 h, blackout 345 h, unplanned outage 7 h, between passes 750 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.762 | 100% | 49.3 (48.2–50.5) | 100.0% | 7.7 mm | 15.4 | 0 h | 24.3 | 10.3 | 0.00 | 0.00 | 64.8 | 0.00 |
| greedy | 0.688 | 100% | 54.9 (50.3–63.2) | 100.0% | 7.4 mm | 16.7 | 0 h | 27.6 | 25.8 | 0.00 | 0.00 | 3.6 | 0.00 |
| random | 0.680 | 100% | 55.6 (49.8–65.5) | 100.0% | 7.4 mm | 16.9 | 0 h | 28.4 | 27.3 | 0.00 | 0.00 | 4.2 | 0.00 |
| FCFS | 0.659 | 100% | 57.2 (51.3–69.1) | 100.0% | 7.4 mm | 16.8 | 0 h | 28.2 | 24.3 | 0.00 | 0.00 | 3.7 | 0.00 |

#### `landing-pad-1.1-small-cascading_faults`

Reference build time 37.5 sols. Conditions per run: storm 131 h, blackout 0 h, unplanned outage 9 h, between passes 1106 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.722 | 100% | 52.0 (50.7–54.0) | 100.0% | 7.6 mm | 16.0 | 0 h | 127.2 | 71.7 | 0.00 | 0.00 | 218.4 | 0.00 |
| greedy | 0.630 | 98% | 58.7 (52.5–69.0) | 98.2% | 7.4 mm | 17.5 | 0 h | 140.6 | 142.8 | 0.00 | 0.00 | 27.8 | 0.00 |
| random | 0.626 | 100% | 60.4 (53.2–72.4) | 100.0% | 7.3 mm | 17.9 | 0 h | 143.9 | 147.9 | 0.00 | 0.00 | 29.1 | 0.00 |
| FCFS | 0.613 | 100% | 61.8 (54.6–80.2) | 100.0% | 7.3 mm | 17.7 | 0 h | 142.7 | 144.5 | 0.00 | 0.00 | 28.5 | 0.00 |

#### `landing-pad-1.1-small-solar_only`

Reference build time 37.5 sols. Conditions per run: storm 126 h, blackout 0 h, unplanned outage 9 h, between passes 1050 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.761 | 100% | 49.4 (48.3–51.0) | 100.0% | 7.6 mm | 15.3 | 2 h | 25.0 | 10.7 | 0.00 | 1.00 | 75.4 | 0.00 |
| greedy | 0.676 | 100% | 55.8 (50.6–64.8) | 100.0% | 7.3 mm | 16.9 | 3 h | 27.4 | 32.1 | 0.00 | 0.00 | 3.6 | 0.00 |
| random | 0.673 | 100% | 56.2 (49.7–67.5) | 100.0% | 7.3 mm | 17.1 | 3 h | 28.4 | 27.0 | 0.00 | 0.00 | 3.3 | 0.00 |
| FCFS | 0.660 | 100% | 57.2 (51.6–76.5) | 100.0% | 7.5 mm | 16.8 | 3 h | 29.1 | 28.6 | 0.00 | 0.00 | 3.9 | 0.00 |

#### `landing-pad-1.1-small-nuclear_heavy`

Reference build time 37.5 sols. Conditions per run: storm 126 h, blackout 0 h, unplanned outage 9 h, between passes 1046 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.764 | 100% | 49.2 (47.7–50.6) | 100.0% | 7.7 mm | 15.3 | 0 h | 24.5 | 10.1 | 0.00 | 0.00 | 75.9 | 0.00 |
| greedy | 0.690 | 100% | 54.7 (50.3–69.5) | 100.0% | 7.4 mm | 16.7 | 0 h | 27.1 | 27.9 | 0.00 | 0.00 | 4.1 | 0.00 |
| random | 0.673 | 100% | 56.2 (50.1–74.4) | 100.0% | 7.3 mm | 17.1 | 0 h | 28.7 | 29.5 | 0.00 | 0.00 | 3.5 | 0.00 |
| FCFS | 0.660 | 100% | 57.3 (50.2–79.8) | 100.0% | 7.4 mm | 16.9 | 0 h | 28.2 | 25.5 | 0.00 | 0.00 | 3.5 | 0.00 |

### Medium

#### `landing-pad-1.1-medium-nominal`

Reference build time 18.8 sols. Conditions per run: storm 48 h, blackout 0 h, unplanned outage 4 h, between passes 524 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.764 | 100% | 24.6 (23.9–25.7) | 100.0% | 7.6 mm | 15.3 | 0 h | 24.2 | 8.7 | 0.00 | 0.42 | 54.2 | 0.00 |
| greedy | 0.683 | 100% | 27.7 (24.6–35.0) | 100.0% | 7.3 mm | 16.9 | 0 h | 28.1 | 14.2 | 0.00 | 0.00 | 3.6 | 0.00 |
| random | 0.678 | 100% | 28.0 (24.6–38.0) | 100.0% | 7.4 mm | 17.1 | 0 h | 28.0 | 14.8 | 0.00 | 0.00 | 3.5 | 0.00 |
| FCFS | 0.659 | 100% | 28.6 (25.5–35.4) | 100.0% | 7.4 mm | 16.8 | 0 h | 26.4 | 14.6 | 0.00 | 0.00 | 3.4 | 0.00 |

#### `landing-pad-1.1-medium-dust_storm`

Reference build time 18.8 sols. Conditions per run: storm 544 h, blackout 0 h, unplanned outage 4 h, between passes 526 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.761 | 100% | 24.7 (24.2–25.3) | 100.0% | 7.6 mm | 15.3 | 0 h | 25.7 | 9.1 | 0.00 | 0.48 | 54.9 | 0.00 |
| greedy | 0.663 | 100% | 28.5 (25.6–37.6) | 100.0% | 7.4 mm | 17.1 | 0 h | 46.5 | 27.2 | 0.00 | 0.00 | 6.6 | 0.00 |
| random | 0.663 | 100% | 28.6 (25.6–37.8) | 100.0% | 7.4 mm | 17.1 | 0 h | 45.8 | 25.8 | 0.00 | 0.00 | 6.2 | 0.00 |
| FCFS | 0.638 | 100% | 29.6 (26.3–36.3) | 100.0% | 7.3 mm | 17.1 | 0 h | 47.0 | 27.7 | 0.00 | 0.00 | 6.2 | 0.00 |

#### `landing-pad-1.1-medium-conjunction`

Reference build time 18.8 sols. Conditions per run: storm 48 h, blackout 345 h, unplanned outage 1 h, between passes 225 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.764 | 100% | 24.6 (24.0–25.8) | 100.0% | 7.6 mm | 15.3 | 0 h | 25.4 | 9.7 | 0.00 | 0.48 | 44.5 | 0.00 |
| greedy | 0.686 | 100% | 27.5 (24.7–33.0) | 100.0% | 7.4 mm | 16.8 | 0 h | 27.2 | 14.6 | 0.00 | 0.00 | 3.5 | 0.00 |
| random | 0.679 | 100% | 28.0 (24.8–39.3) | 100.0% | 7.3 mm | 17.0 | 0 h | 27.9 | 14.7 | 0.00 | 0.00 | 3.4 | 0.00 |
| FCFS | 0.653 | 100% | 29.0 (25.7–42.2) | 100.0% | 7.3 mm | 17.0 | 0 h | 26.7 | 14.5 | 0.00 | 0.00 | 3.3 | 0.00 |

#### `landing-pad-1.1-medium-cascading_faults`

Reference build time 18.8 sols. Conditions per run: storm 51 h, blackout 0 h, unplanned outage 4 h, between passes 551 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.726 | 100% | 25.9 (25.0–26.5) | 100.0% | 7.7 mm | 15.9 | 0 h | 124.1 | 60.0 | 0.00 | 0.56 | 184.0 | 0.00 |
| greedy | 0.654 | 100% | 28.9 (25.9–35.4) | 100.0% | 7.4 mm | 17.4 | 0 h | 141.4 | 92.1 | 0.00 | 0.00 | 27.0 | 0.00 |
| random | 0.655 | 100% | 28.8 (25.8–33.8) | 100.0% | 7.4 mm | 17.3 | 0 h | 136.6 | 87.3 | 0.00 | 0.00 | 25.9 | 0.00 |
| FCFS | 0.625 | 100% | 30.3 (26.8–41.3) | 100.0% | 7.3 mm | 17.6 | 0 h | 141.7 | 91.1 | 0.00 | 0.00 | 27.9 | 0.00 |

#### `landing-pad-1.1-medium-solar_only`

Reference build time 18.8 sols. Conditions per run: storm 53 h, blackout 0 h, unplanned outage 4 h, between passes 565 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.708 | 100% | 26.5 (25.4–29.1) | 100.0% | 7.6 mm | 15.3 | 130 h | 24.1 | 9.7 | 0.00 | 1.00 | 56.1 | 0.00 |
| greedy | 0.649 | 100% | 29.0 (26.4–35.8) | 100.0% | 7.5 mm | 16.5 | 143 h | 26.3 | 13.0 | 0.00 | 0.00 | 2.8 | 0.00 |
| random | 0.634 | 100% | 29.8 (26.1–38.3) | 100.0% | 7.4 mm | 16.9 | 147 h | 26.3 | 15.7 | 0.00 | 0.00 | 3.2 | 0.00 |
| FCFS | 0.618 | 100% | 30.6 (27.0–39.4) | 100.0% | 7.4 mm | 16.8 | 144 h | 27.3 | 15.0 | 0.00 | 0.00 | 3.2 | 0.00 |

#### `landing-pad-1.1-medium-nuclear_heavy`

Reference build time 18.8 sols. Conditions per run: storm 48 h, blackout 0 h, unplanned outage 4 h, between passes 523 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.766 | 100% | 24.5 (24.0–25.2) | 100.0% | 7.6 mm | 15.3 | 0 h | 24.8 | 9.3 | 0.00 | 0.00 | 55.2 | 0.00 |
| greedy | 0.682 | 100% | 27.7 (25.0–36.2) | 100.0% | 7.3 mm | 16.9 | 0 h | 27.6 | 12.6 | 0.00 | 0.00 | 3.2 | 0.00 |
| random | 0.677 | 100% | 28.0 (24.6–37.6) | 100.0% | 7.4 mm | 17.1 | 0 h | 28.4 | 15.5 | 0.00 | 0.00 | 3.5 | 0.00 |
| FCFS | 0.659 | 100% | 28.6 (25.5–33.2) | 100.0% | 7.4 mm | 16.8 | 0 h | 27.2 | 14.8 | 0.00 | 0.00 | 3.5 | 0.00 |

### Large

#### `landing-pad-1.1-large-nominal`

Reference build time 11.3 sols. Conditions per run: storm 28 h, blackout 0 h, unplanned outage 3 h, between passes 346 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.692 | 100% | 16.3 (16.0–17.3) | 100.0% | 7.6 mm | 15.4 | 182 h | 24.8 | 9.9 | 0.00 | 1.02 | 46.7 | 0.00 |
| greedy | 0.627 | 100% | 18.1 (16.2–20.9) | 100.0% | 7.4 mm | 16.7 | 187 h | 25.2 | 9.7 | 0.00 | 0.00 | 2.7 | 0.00 |
| random | 0.628 | 100% | 18.0 (16.2–21.8) | 100.0% | 7.4 mm | 16.7 | 182 h | 25.9 | 10.7 | 0.00 | 0.00 | 3.2 | 0.00 |
| FCFS | 0.606 | 100% | 18.7 (16.5–24.1) | 100.0% | 7.3 mm | 16.9 | 183 h | 26.7 | 12.8 | 0.00 | 0.00 | 3.6 | 0.00 |

#### `landing-pad-1.1-large-dust_storm`

Reference build time 11.3 sols. Conditions per run: storm 388 h, blackout 0 h, unplanned outage 3 h, between passes 376 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.639 | 100% | 17.6 (17.1–18.4) | 100.0% | 7.6 mm | 15.4 | 311 h | 26.0 | 9.6 | 0.00 | 1.00 | 47.6 | 0.00 |
| greedy | 0.570 | 100% | 19.9 (17.7–25.9) | 100.0% | 7.3 mm | 17.1 | 299 h | 45.1 | 21.0 | 0.00 | 0.00 | 5.8 | 0.00 |
| random | 0.569 | 100% | 20.0 (17.6–24.5) | 100.0% | 7.3 mm | 17.2 | 301 h | 46.0 | 22.7 | 0.00 | 0.00 | 6.6 | 0.00 |
| FCFS | 0.553 | 100% | 20.6 (18.2–28.6) | 100.0% | 7.3 mm | 17.2 | 298 h | 45.1 | 22.1 | 0.00 | 0.00 | 5.9 | 0.00 |

#### `landing-pad-1.1-large-conjunction`

Reference build time 11.3 sols. Conditions per run: storm 28 h, blackout 329 h, unplanned outage 1 h, between passes 63 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.690 | 100% | 16.3 (15.9–17.1) | 100.0% | 7.6 mm | 15.4 | 177 h | 24.4 | 10.1 | 0.00 | 1.02 | 36.0 | 0.00 |
| greedy | 0.621 | 100% | 18.3 (16.1–23.2) | 100.0% | 7.4 mm | 16.9 | 185 h | 26.3 | 10.1 | 0.00 | 0.00 | 2.7 | 0.00 |
| random | 0.627 | 100% | 18.1 (16.2–21.8) | 100.0% | 7.4 mm | 16.7 | 186 h | 25.3 | 10.2 | 0.00 | 0.00 | 3.0 | 0.00 |
| FCFS | 0.607 | 100% | 18.7 (16.7–22.7) | 100.0% | 7.3 mm | 16.9 | 186 h | 25.9 | 11.7 | 0.00 | 0.00 | 3.4 | 0.00 |

#### `landing-pad-1.1-large-cascading_faults`

Reference build time 11.3 sols. Conditions per run: storm 29 h, blackout 0 h, unplanned outage 3 h, between passes 361 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.664 | 100% | 17.0 (16.5–17.5) | 100.0% | 7.6 mm | 15.9 | 173 h | 122.6 | 58.9 | 0.00 | 1.10 | 170.6 | 0.00 |
| greedy | 0.599 | 100% | 18.9 (16.9–23.8) | 100.0% | 7.4 mm | 17.5 | 168 h | 139.6 | 77.9 | 0.00 | 0.00 | 26.4 | 0.00 |
| random | 0.591 | 100% | 19.2 (17.0–22.7) | 100.0% | 7.4 mm | 17.7 | 169 h | 140.0 | 73.8 | 0.00 | 0.00 | 25.3 | 0.00 |
| FCFS | 0.587 | 100% | 19.3 (17.3–23.8) | 100.0% | 7.4 mm | 17.4 | 165 h | 135.6 | 75.4 | 0.00 | 0.00 | 26.4 | 0.00 |

#### `landing-pad-1.1-large-solar_only`

Reference build time 11.3 sols. Conditions per run: storm 44 h, blackout 0 h, unplanned outage 4 h, between passes 485 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.496 | 100% | 22.8 (21.8–25.9) | 100.0% | 7.7 mm | 15.5 | 448 h | 24.4 | 14.9 | 0.00 | 1.02 | 54.8 | 0.00 |
| greedy | 0.450 | 100% | 25.2 (22.2–32.1) | 100.0% | 7.4 mm | 16.9 | 502 h | 25.5 | 18.3 | 0.00 | 0.00 | 3.2 | 0.00 |
| random | 0.451 | 100% | 25.2 (22.4–31.7) | 100.0% | 7.4 mm | 16.9 | 503 h | 25.7 | 19.0 | 0.00 | 0.00 | 3.3 | 0.00 |
| FCFS | 0.438 | 100% | 26.0 (22.4–34.2) | 100.0% | 7.3 mm | 17.1 | 508 h | 26.8 | 22.6 | 0.00 | 0.00 | 3.0 | 0.00 |

#### `landing-pad-1.1-large-nuclear_heavy`

Reference build time 11.3 sols. Conditions per run: storm 22 h, blackout 0 h, unplanned outage 3 h, between passes 315 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.762 | 100% | 14.8 (14.3–15.5) | 100.0% | 7.7 mm | 15.3 | 4 h | 24.0 | 8.8 | 0.00 | 0.94 | 44.8 | 0.00 |
| greedy | 0.676 | 100% | 16.8 (14.7–21.2) | 100.0% | 7.3 mm | 16.9 | 9 h | 26.7 | 11.7 | 0.00 | 0.00 | 3.3 | 0.00 |
| random | 0.674 | 100% | 16.9 (14.7–20.4) | 100.0% | 7.3 mm | 16.9 | 9 h | 26.9 | 10.8 | 0.00 | 0.00 | 3.3 | 0.00 |
| FCFS | 0.656 | 100% | 17.3 (15.1–22.1) | 100.0% | 7.3 mm | 17.0 | 6 h | 25.5 | 9.8 | 0.00 | 0.00 | 2.6 | 0.00 |

### Fleet design track: crew 2.1.1.10

#### `landing-pad-1.1-crew.2.1.1.10-nominal`

Reference build time 10.2 sols. Conditions per run: storm 27 h, blackout 0 h, unplanned outage 3 h, between passes 344 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.634 | 100% | 16.1 (15.9–16.9) | 100.0% | 7.7 mm | 15.2 | 176 h | 23.9 | 10.3 | 0.00 | 1.00 | 46.7 | 0.00 |
| greedy | 0.571 | 100% | 18.1 (16.2–22.6) | 100.0% | 7.3 mm | 16.7 | 178 h | 27.5 | 13.1 | 0.00 | 0.00 | 3.0 | 0.00 |

#### `landing-pad-1.1-crew.2.1.1.10-dust_storm`

Reference build time 10.2 sols. Conditions per run: storm 384 h, blackout 0 h, unplanned outage 3 h, between passes 372 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.586 | 100% | 17.5 (16.9–18.4) | 100.0% | 7.7 mm | 15.2 | 307 h | 24.9 | 12.3 | 0.00 | 1.04 | 50.6 | 0.00 |
| greedy | 0.529 | 100% | 19.5 (17.6–23.0) | 100.0% | 7.4 mm | 16.8 | 305 h | 44.5 | 29.6 | 0.00 | 0.00 | 6.2 | 0.00 |

#### `landing-pad-1.1-crew.2.1.1.10-conjunction`

Reference build time 10.2 sols. Conditions per run: storm 27 h, blackout 326 h, unplanned outage 1 h, between passes 63 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.632 | 100% | 16.2 (15.9–17.4) | 100.0% | 7.7 mm | 15.2 | 168 h | 23.7 | 11.2 | 0.00 | 1.00 | 36.7 | 0.00 |
| greedy | 0.575 | 100% | 17.9 (16.1–21.9) | 100.0% | 7.4 mm | 16.6 | 173 h | 27.6 | 15.7 | 0.00 | 0.00 | 3.4 | 0.00 |

#### `landing-pad-1.1-crew.2.1.1.10-cascading_faults`

Reference build time 10.2 sols. Conditions per run: storm 29 h, blackout 0 h, unplanned outage 3 h, between passes 359 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.608 | 100% | 16.9 (16.4–18.3) | 100.0% | 7.6 mm | 15.8 | 160 h | 123.5 | 63.8 | 0.00 | 1.12 | 179.4 | 0.00 |
| greedy | 0.553 | 100% | 18.8 (16.8–26.3) | 100.0% | 7.4 mm | 17.3 | 149 h | 139.9 | 97.6 | 0.00 | 0.00 | 25.7 | 0.00 |

#### `landing-pad-1.1-crew.2.1.1.10-solar_only`

Reference build time 10.2 sols. Conditions per run: storm 43 h, blackout 0 h, unplanned outage 4 h, between passes 481 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.454 | 100% | 22.6 (21.6–25.6) | 100.0% | 7.7 mm | 15.3 | 343 h | 23.8 | 32.6 | 0.00 | 1.02 | 59.7 | 0.00 |
| greedy | 0.426 | 100% | 24.2 (21.9–29.3) | 100.0% | 7.4 mm | 16.4 | 387 h | 25.2 | 26.3 | 0.00 | 0.00 | 3.3 | 0.00 |

#### `landing-pad-1.1-crew.2.1.1.10-nuclear_heavy`

Reference build time 10.2 sols. Conditions per run: storm 23 h, blackout 0 h, unplanned outage 3 h, between passes 318 h.

| Coordinator | Headline | Success | Sols: mean (min–max) | In spec | RMS error | MWh per pad | Bank empty | Faults | Robot-h lost | Robots lost | Approval requests | To Earth | Violations |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| ArcadiaOS | 0.686 | 100% | 14.9 (14.5–15.5) | 100.0% | 7.7 mm | 15.2 | 0 h | 24.6 | 10.4 | 0.00 | 0.00 | 48.8 | 0.00 |
| greedy | 0.622 | 100% | 16.6 (14.7–21.2) | 100.0% | 7.4 mm | 16.4 | 0 h | 26.7 | 14.7 | 0.00 | 0.00 | 3.2 | 0.00 |

## Reproduce

The results come from:

| Software | Version | Release |
|---|---|---|
| ArcadiaBench (`arcadiabench` and the baselines) | 1.1.0 | tag `v1.1.0` |
| ArcadiaOS (`arcadiaos`) | 1.1.0 | tag `v1.1.0` |
| arcadia-protocol | 1.1.0 | tag `v1.1.0` |
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
  target/release/arcadiabench suite --catalog 1.1 --seeds 1-50 --out "$RUNS/$c" -- "$coord"
done

# The fleet design track: ArcadiaOS designs a crew for the Medium outpost and 10 t.
target/release/arcadiabench scenario --catalog 1.1 --preset medium --seed 1 --goal > "$RUNS/scenario.jsonl"
$ARCADIAOS design-crew --scenario "$RUNS/scenario.jsonl" --budget-kg 10000 > results/v1.1/fleet/crew.json
for c in arcadiaos baseline-greedy; do
  if [ "$c" = arcadiaos ]; then coord=$ARCADIAOS; else coord=target/release/$c; fi
  target/release/arcadiabench suite --catalog 1.1 --crew results/v1.1/fleet/crew.json --seeds 1-50 --out "$RUNS/fleet-$c" -- "$coord"
done

# One scorecard per coordinator and scenario, and the report.
#   cp "$RUNS/<suite>/<scenario>/scorecard.json" results/v1.1/scorecards/<coordinator>/<scenario>.json
target/release/arcadiabench score --json "$RUNS"/*/*/metrics.jsonl > results/v1.1/results.json
```

`ARCADIAOS` names the ArcadiaOS binary and defaults to `../arcadiaos/target/release/arcadiaos`. Pass the script a second argument to write the files somewhere else, for example `scripts/results.sh v1.1 /tmp/again` to compare a fresh run with these. Each `suite` of the Standard track does 900 runs, one coordinator process per run, in about a minute on the machine above.

## Files

- [`results.json`](results.json): every scorecard and each coordinator's resilience, as a report (`schemas/scorecard.schema.json#/$defs/Report`).
- [`scorecards/<coordinator>/1.1-<preset>-<variant>.json`](scorecards/): one scorecard per coordinator and scenario (the schema's root). The fleet design track's are `1.1-crew.2.1.1.10-<variant>.json`.
- [`fleet/crew.json`](fleet/crew.json): the crew ArcadiaOS designed (`schemas/crew.schema.json`).
- [`replays/`](replays/): four ArcadiaOS runs as gzipped event logs (protocol 1.1.0 `event_log.schema.json`), for the replay viewer. [`docs/demos.md`](../../docs/demos.md) describes them.

No other raw logs are kept. `tests/results.rs` checks that these files match their schemas, that the scorecards agree with `results.json`, and that the replays are valid logs within the size limit.
