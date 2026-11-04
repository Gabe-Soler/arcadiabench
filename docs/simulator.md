# ArcadiaBench simulator reference

This document gives the numbers behind the interface. The protocol's [semantics](https://github.com/Gabe-Soler/arcadia-protocol/blob/v0.2.0/docs/semantics.md) fix the structure: what happens, in what order, and what a coordinator sees. This document gives the parameter values ArcadiaBench uses and the functional forms they feed. Every value lives in [`src/sim/params.rs`](../src/sim/params.rs), with a doc comment on each field.

All values are placeholders. They are chosen to be physically plausible, to make a 20 m pad buildable in a few sols, and to make faults, comms and Earth's oversight matter without crippling a crew. Sourcing realistic numbers is an open question in the project outline.

## Contents

1. [The scenario catalog](#1-the-scenario-catalog)
2. [Time and environment](#2-time-and-environment)
3. [Terrain](#3-terrain)
4. [Actions: durations and energy](#4-actions-durations-and-energy)
5. [Sensing and telemetry](#5-sensing-and-telemetry)
6. [Site power](#6-site-power)
7. [Wear](#7-wear)
8. [Failure processes](#8-failure-processes)
9. [Repair, tow, cannibalize and service](#9-repair-tow-cannibalize-and-service)
10. [Cold nights](#10-cold-nights)
11. [The Earth link](#11-the-earth-link)
12. [Scripted mission control](#12-scripted-mission-control)
13. [Authority enforcement](#13-authority-enforcement)
14. [Coverage and relays](#14-coverage-and-relays)
15. [The landing-pad scenarios](#15-the-landing-pad-scenarios)
16. [Randomness and reproducibility](#16-randomness-and-reproducibility)
17. [Session limits and the event log](#17-session-limits-and-the-event-log)
18. [Calibration](#18-calibration)

## 1. The scenario catalog

A scenario is a **preset** (the crew and its outpost) combined with a **variant** (what the run throws at the crew). Every preset combines with every variant, and the scenario is named `landing-pad-<preset>-<variant>`.

- **Command line:** `arcadiabench run --preset <name> --variant <name>`. `bench`, `suite` and the Python package take the same names.
- **Rust:** `Params::scenario(Preset::…, Variant::…)`. `Params::variant(Variant::…)` and the constructors `Params::nominal()`, `Params::dust_storm()`, `Params::conjunction()`, `Params::cascading_faults()`, `Params::solar_only()` and `Params::nuclear_heavy()` give the Small preset.
- **Defaults:** `small` and `nominal`.

Every scenario has the same site, the same landing-pad goal and the same base station (§15), and a seed gives the same terrain, weather and Earth link in every preset.

### Presets

The Standard track presets are defined in [`src/presets.rs`](../src/presets.rs).

| Preset | Robots | Crew | Stations (6 kW, 2 slots each) | Depots |
|---|---|---|---|---|
| `small` | 4 | 1 scout, 1 excavator, 1 hauler, 1 compactor-sinterer | `st-1` (10, 10) | `dp-1` (8, 14) |
| `medium` | 10 | 2 scouts, 2 excavators, 2 haulers, 4 compactor-sinterers | + `st-2` (49, 10) | + `dp-2` (51, 14) |
| `large` | 20 | 4 scouts, 4 excavators, 4 haulers, 8 compactor-sinterers | + `st-3` (10, 30), `st-4` (49, 30) | + `dp-3` (8, 26), `dp-4` (51, 26) |

Why this capability mix:

- **The same four designs.** Every robot is one of the Small crew's four designs, with unchanged specs (§15). A rate, a battery or a speed means the same in every preset, so results differ only by crew size and coordination.
- **Weighted toward sintering.** Sintering is the slowest step and dominates the work. On Small, ArcadiaOS keeps its compactor-sinterer busy about 20 of the crew's 33 busy robot-hours, including the charging that sintering's 24 kWh needs. So beyond the minimal Small crew of one robot per design, compactor-sinterers make up 40% of the crew.
- **No single points of failure.** Every other design is doubled in Medium and doubled again in Large. Every capability is carried by at least two robots in Medium and four in Large, so no single breakdown strands a capability.
- **Nested crews.** Medium is the Small crew plus six robots, and Large is the Medium crew plus ten, with the same ids (`sc-1`, `ex-1`, `hl-1`, `cp-1`, then `sc-2`, ...). A coordinator that only knows the Small crew still runs on every preset.

The outpost scales with the crew:

- **Stations and depots.** Medium mirrors the Small station and depot across the base station, to the east of the outpost. Large adds a station and a depot on each side of the pad.
- **Constant ratios.** Every station keeps the Small station's 3 kW per slot. That makes 0.5 slots per robot in Small and 0.4 in Medium and Large, and 4 to 5 robots per depot.
- **Everything in coverage.** Every station, depot and start position stands inside the base station's coverage and off the pad.
- **The same base station.** It is unchanged in every preset, so every crew meets the same coverage problem at the berm.
- **The same power plant.** The power system is the variant's (§6) whatever the preset. A bigger crew shares the same supply: Large's eight charge slots can draw 24 kW, more than the 10 kW hybrid baseline gives at night.

Robots start in rows of ten eastward from (12, 10): Small at (12–15, 10), Medium at (12–21, 10), and Large adds (12–21, 11).

The presets need no randomness of their own. Small is exactly the week-6 scenario, apart from its name, and `tests/regression.rs` checks this byte for byte.

### Variants

| Variant | What changes from `nominal` |
|---|---|
| `nominal` | Hybrid power and the nominal fault model. |
| `dust_storm` | A severe regional dust storm from the start of the run: its front arrives in the first 0.02 sol (half an hour), opacity peaks 0.03 sol later at tau 3.5–5, and the storm lasts 4–8 sols (§2). |
| `conjunction` | A 14-sol solar-conjunction blackout of the Earth link that starts 0.01–0.03 sols (15–45 minutes) into the run, with the one-way delay near its 24-minute maximum (§11). |
| `cascading_faults` | Every wear rate and the wear hazard ×4, the random hazard ×5, the storm sensor hazard ×3, the stuck rate ×3, and only 40% of disabling faults field repairable. |
| `solar_only` | No fission; 30 kW solar peak; 150 kWh storage (§6). |
| `nuclear_heavy` | 15 kW fission; 5 kW solar peak; 50 kWh storage (§6). |

Unplanned link outages (§11) happen in every variant. `dust_storm`, `conjunction` and `cascading_faults` are the stress variants that resilience is measured against ([`docs/scoring.md`](scoring.md)). `solar_only` and `nuclear_heavy` are alternative power architectures.

**Stress timing.** A stress variant must stress every preset, and the presets' builds differ tenfold in length: a Small crew needs most of a sol, a Large one under 0.1 sol. So the dust storm and the blackout begin within the first 0.03 sol, on a time drawn from their usual streams:

- **A seeded start inside the run, not a run that begins mid-stress.** Semantics §8.1 has the blackout start on a seeded sol inside the run. Starting it a little after `t = 0` keeps that: the run opens with the link up, so a coordinator can send its opening report, and then the link goes down with a `comms` event, a decision point carrying `expected_restore_t`, which is part of what the variant tests and what a replay shows. The storm front likewise arrives as a `weather` event during the run.
- **Early enough for the fastest build.** ArcadiaOS finishes Large in 0.077–0.089 sols (seeds 1–50). A blackout starting by 0.03 sol covers at least 60% of that build and essentially all of every Small and Medium build. The storm front arrives by 0.02 sol and peaks at most 0.03 sol later, so even a Large build spends three quarters of its time in the storm and meets it at full strength before the end.
- **Same draws, new range.** Only the ranges the draws map onto changed, so every seed still gets the same storm length and peak, and the same unplanned outages outside the blackout.

`t = 0` is local midnight and sunrise is at sol phase 0.25, so Medium and Large builds happen entirely in the dark: there the storm brings sensor faults and wear, not lost sunlight.

`Params { faults_enabled: false, .. }` turns off wear growth and every random failure process: the core physics without faults. Scripted incidents still happen. `scripted` holds incidents forced at fixed times, for tests and demo scenarios: a fault on a given component, or getting stuck. It is empty in every built-in variant.

## 2. Time and environment

Sim time is kept in integer milliseconds. One sol is 88,775 s, and `t = 0` is local midnight.

- **Daylight** is from sol phase 0.25 to 0.75. Clear-sky solar output is `solar_peak_w × sin(π (phase − 0.25) / 0.5)` in daylight, and 0 at night.
- **Temperature** is `−50 + 30 sin(2π (phase − 0.375))` °C. That is about −80 °C before dawn and −20 °C in mid-afternoon.

| Parameter | Value | Rationale |
|---|---|---|
| `clear_tau` | 0.5 | Typical clear-season dust opacity at mid-latitudes. |
| `storm_chance_per_sol` | 0.03 | About two regional storms in a 60-sol run. |
| `storm_sols` | 2–10 sols | Regional storms last days, not months. |
| `storm_peak_tau` | 2–4 | Severe regional storms; global storms exceed this. |
| `dust_extinction` | 0.6 | Solar transmittance `exp(−0.6 (tau − clear_tau))`: about 30% of clear-sky output at tau 2.5. |

Storms come from the `weather` stream, at most one at a time. Each sol with no storm raging starts one with probability `storm_chance_per_sol`, at a uniform time within the sol. Opacity ramps linearly from clear to the peak over the first fifth of the storm, then falls linearly back to clear at its end.

**The `dust_storm` variant's storm** is drawn from its own `dust_storm` stream, so the seasonal storms it leaves in place are the same as in every other variant. Seasonal storms that would overlap it are dropped. Its front arrives at a uniform time in `dust_storm_start_sols`, and opacity climbs linearly to the peak over `dust_storm_rise_sols`, then falls linearly back to clear at its end.

| Parameter | Value | Rationale |
|---|---|---|
| `dust_storm_start_sols` | 0.0–0.02 sols | Within half an hour of the start, so the storm covers every preset's build: a Large crew can finish in under 0.1 sol (§1, stress timing). |
| `dust_storm_rise_sols` | 0.03 sols | A front blows in over the better part of an hour, so even the fastest build meets the storm near its peak. |
| `dust_storm_sols` | 4–8 sols | Longer than most seasonal storms. |
| `dust_storm_peak_tau` | 3.5–5 | Severe: solar transmittance falls to 7–17% of clear sky. |

The storm brings everything a seasonal storm does: storm sensor faults on robots that scan or drive, faster wear, more dust exposure, and less solar power. A build that finishes during the first night (sunrise is at sol phase 0.25) meets only the first three.

## 3. Terrain

The `terrain` stream generates the site. It is a gently tilted plain with three octaves of smooth value noise, regolith depth, density and cohesion fields, and a sparse power-law boulder field.

| Parameter | Value | Rationale |
|---|---|---|
| `base_elevation_m` | −4102 m | Arcadia Planitia sits about 4 km below the datum. |
| `tilt_sigma` | 0.004 m/m | A smooth plain, well under 1°. |
| `noise_octaves` | (16 cells, 0.30 m), (8, 0.12), (4, 0.04) | Gentle relief, so the pad needs real cut and fill but is buildable. |
| `regolith_depth_m` | 0.4–1.5 m | Loose cover above ice-cemented ground. |
| `regolith_density_kg_m3` | 1400–1700 | Bulk density of Martian soil. |
| `regolith_cohesion_kpa` | 0.5–3.0 | The range reported for Martian soils. |
| `boulders_per_cell` | 0.06 | Arcadia has low rock abundance. |
| `boulder_d_min_m`, `boulder_d_max_m` | 0.1 m, 1.2 m | Diameters are `d_min · u^(−1/alpha)`, capped at `d_max`. |
| `boulder_alpha` | 2.5 | Power-law size distribution. |
| `rock_density_kg_m3` | 2800 | Basalt. |
| `orbital_elevation_noise_m` | 0.25 m | HiRISE-DTM-like accuracy of the orbital prior, from the `orbital` stream. |

## 4. Actions: durations and energy

`LN(σ)` is multiplicative lognormal noise with mean 1, `exp(σZ − σ²/2)`. A step's energy is drawn when it starts. Wear and fault multipliers (§7) are applied at that moment: `speed` is the speed factor, `drive_e` the drive energy factor, `rate` and `work_e` the work factors (or the scan rate factor), and `e` the battery energy factor.

| Step | Duration | Energy |
|---|---|---|
| Drive | `dist / (max_speed · speed) · LN(travel σ)` | `(dist · drive_energy_wh_per_m · drive_e + duration · idle_power / 3600) · e` |
| Dig, grade, compact, sinter | `max(units / (cap rate · rate) · LN(work σ), min_work_s)` | `(units · energy_wh_per_unit · work_e + duration · idle_power / 3600) · e` |
| Scan | as for work, with the scan rate factor | `(units · energy_wh_per_unit + duration · idle_power / 3600) · e` |
| Load, dump | `kg / (haul rate · rate) · LN(work σ)` | `(kg · energy_wh_per_unit · work_e + duration · idle_power / 3600) · e` |
| Wait | exactly `duration_s` | `duration · idle_power / 3600 · e` |

| Parameter | Value | Rationale |
|---|---|---|
| `work_duration_sigma` | 0.15 | Work-rate variability of about 15%. |
| `travel_duration_sigma` | 0.05 | Driving is more predictable than digging. |
| `min_work_s` | 5 s | Positioning overhead per cell. |
| `dig_cut_noise_m` | 0.02 m | A dig stops within a couple of centimetres of its target. |
| `grade_max_delta_m` | 0.15 m | Grading is fine levelling; bigger differences need dig or haul. |
| `grade_noise_m` | 0.005 m | A graded surface is within millimetres. |
| `compact_settle_m`, `compact_settle_noise_m` | 0.01 m, 0.002 m | Compaction settles the surface by about a centimetre. |

A step cut short (cancel, a disabling fault, getting stuck) returns `energy · (time left / duration)` to the battery. Wear grows for the fraction done.

## 5. Sensing and telemetry

| Parameter | Value | Rationale |
|---|---|---|
| `scan_elevation_noise_m` | 0.01 m | Lidar ranging accuracy. |
| `scan_depth_noise_m` | 0.05 m | Ground-penetrating radar is coarser. |
| `scan_density_noise_frac` | 3% | Density from penetrometry. |
| `scan_min_boulder_m` | 0.1 m | The smallest boulder a scan resolves. |
| `scan_boulder_noise_frac` | 5% | Size estimates from imagery. |
| `telemetry_battery_noise_frac` | 0.5% of capacity | State-of-charge estimation error. |

**Health telemetry** is computed from hidden wear and faults, with Gaussian noise added in observations from the `health` stream. `truth` records are noise-free.

| Field | Noise-free value | Noise σ |
|---|---|---|
| `wheel_slip` | `slip_base + slip_per_wear · w_wheels + slip_fault` (if `wheels` has a fault), clamped to [0, 1] | `telemetry_slip_noise` |
| `motor_current_ratio` | `1 + current_per_wear · w_drive_motor + current_fault` (if `drive_motor` has a fault) | `telemetry_current_noise` |
| `dust_coverage` | `dust_per_joint_wear · w_joints + dust_per_sensor_wear · w_sensors`, clamped to [0, 1] | `telemetry_dust_noise` |
| `temperature_c` | ambient, plus `heater_offset_c` while the heater runs (§10) | `telemetry_temp_noise_c` |

| Parameter | Value | Rationale |
|---|---|---|
| `slip_base` | 0.02 | A healthy rover on loose ground slips a little. |
| `slip_per_wear`, `slip_fault` | 0.3, 0.15 | Worn wheels slip up to 0.32; a wheel fault is obvious. |
| `current_per_wear`, `current_fault` | 0.6, 0.3 | A worn motor draws up to 1.6× nominal current. |
| `dust_per_joint_wear`, `dust_per_sensor_wear` | 0.6, 0.4 | Dust coverage reflects dust-driven wear. |
| `telemetry_slip_noise` | 0.01 | Wear shows in trends rather than in single readings. |
| `telemetry_current_noise` | 0.02 | As above. |
| `telemetry_dust_noise` | 0.02 | As above. |
| `telemetry_temp_noise_c` | 0.5 °C | Thermistor accuracy. |

## 6. Site power

Supply is `fission_w + solar_peak_w × clear-sky curve × transmittance`. Demand is the essential load plus station charging, at `max_power_w / slots` per occupied slot.

The grid is settled at every power tick, at every environment event, and whenever charging starts or stops. Between two settle points, solar output is taken at the midpoint, so storage changes linearly and its minimum falls on a settle point.

When storage runs dry, the essential load is served first. Station charging gets `max(0, supply − essential + storage / interval) / requested` of its power. A shortfall of the essential load itself has no effect beyond the `deficit_s` metric.

| Mode | `fission_w` | `solar_peak_w` | `bank_capacity_wh` | `essential_load_w` | What dominates |
|---|---|---|---|---|---|
| `hybrid` (default) | 10,000 | 20,000 | 100,000 | 2,000 | Timing heavy work to solar peaks above a steady floor. |
| `solar_only` | 0 | 30,000 | 150,000 | 2,000 | Day-night swings and storms: about 235 kWh of clear-sky solar a sol, about 25 kWh of essential load every night. |
| `nuclear_heavy` | 15,000 | 5,000 | 50,000 | 2,000 | Steady supply. The 6 kW station is the limit, not the grid. |

| Parameter | Value | Rationale |
|---|---|---|
| `bank_initial_frac` | 0.6 | Storage starts part-charged. |
| `bank_low_frac` | 0.2 | The low-storage alert threshold. |
| `power_tick_s` | 900 s | Accounting granularity. It is not a decision point; alerts are checked at each tick. |

Mobile chargers deliver their charge capability `rate × 3600` W from their tank. In the default crew, `hl-1` delivers 1.8 kW from an 8 kWh tank. Transfers are lossless, and tanks refill when the charger tops up at a station without `until_wh`.

## 7. Wear

Each robot has hidden wear in [0, 1] for `wheels`, `drive_motor`, `joints`, `tool`, `sensors` and `battery`. Those are the only component names used in faults.

**Growth.** Wear is applied when a step ends, pro rata if the step is cut short. Idle drain adds battery wear as it is settled. Everything is multiplied by `storm_wear_factor` when a storm was active at the step's start.

| Component | Growth | Rate |
|---|---|---|
| `wheels` | per km driven | `wear_wheels_per_km` = 0.08 |
| `drive_motor` | per km driven | `wear_drive_motor_per_km` = 0.06 |
| `tool` | per hour of work (scans excluded) | `wear_tool_per_work_h` = 0.02 |
| `joints` | per hour of work, plus per dust-hour | `wear_joints_per_work_h` = 0.012, `wear_joints_per_dust_h` = 0.004 |
| `sensors` | per dust-hour | `wear_sensors_per_dust_h` = 0.006 |
| `battery` | per kWh drawn | `wear_battery_per_kwh` = 0.006 |

A dust-hour is an hour of driving or working outdoors, weighted by `tau / clear_tau` at the step's start. `storm_wear_factor` = 2.0. With these rates an excavator digging through a whole sol reaches about 0.5 tool wear, and a hauler driving 5 km reaches about 0.4 wheel wear.

**Effects.** Multipliers are applied when a step starts. Each is linear in wear; a degraded fault on the component adds its penalty.

| Factor | Formula | At wear 1 with a fault |
|---|---|---|
| Speed | `(1 − 0.3 · w_wheels) × 0.7 if wheels faulty` | ×0.49 |
| Drive energy | `(1 + 0.3 · w_drive_motor) × 1.4 if drive_motor faulty` | ×1.82 |
| Work rate (not scans) | `(1 − 0.2 · w_joints)(1 − 0.2 · w_tool) × 0.75 per faulty joints or tool` | ×0.36 |
| Work energy (not scans) | `(1 + 0.2 · w_tool) × 1.25 if tool faulty` | ×1.5 |
| Scan rate | `(1 − 0.3 · w_sensors) × 0.5 if sensors faulty` | ×0.35 |
| All energy | `(1 + 0.15 · w_battery) × 1.25 if battery faulty` | ×1.44 |

The parameters are `wear_speed_loss`, `wear_drive_energy_gain`, `wear_work_rate_loss`, `wear_work_energy_gain`, `wear_scan_rate_loss` and `wear_energy_gain`, plus the `degraded_*_factor` set. Estimates in `command_accepted` ignore all of these, so actual durations and energy against the estimate reveal wear.

## 8. Failure processes

A robot is exposed while driving (towing included), during work steps and scans, and while holding as a relay. When an exposed step starts, each process samples the time to its next event from an exponential distribution, using the hazard at that moment and its own RNG stream. An event that would land after the step ends is dropped. A new exposure invalidates events left over from earlier steps. After a degraded fault, the process that caused it samples again for the rest of the step.

| Process | Stream | Hazard per exposed hour | Component | P(disabled) |
|---|---|---|---|---|
| Wear | `wear_faults` | `wear_hazard_per_h · w^wear_hazard_exponent` = `0.05 · w³` per component | in proportion to the component hazards | `p_disabled_wear` = 0.4 |
| Random | `random_faults` | `random_fault_per_h` = 0.008 | uniform | `p_disabled_random` = 0.3 |
| Storm | `storm_faults` | `storm_sensor_fault_per_h · (tau − clear_tau)` = `0.02 · (tau − 0.5)`, while scanning or driving in a storm | `sensors` | `p_disabled_environmental` = 0.2 |

A disabling fault is field repairable with probability `p_field_repairable` = 0.6. Degraded faults always are. A robot has at most one fault per component: a new degraded fault on a component that already has one is ignored, and a disabling one replaces it.

The wear hazard is steep on purpose. At wear 0.3 a component fails about once per 740 exposed hours; at 0.9, about once per 27. Predictive maintenance pays off.

**Getting stuck** (stream `stuck`) is a condition, not a fault. A drive is walked one cell length at a time; each sample adds `stuck_per_km × softness × step km` to a cumulative hazard, and the robot gets stuck where the total passes a threshold drawn once per drive.

`softness = clamp((depth − soft_depth_m) / soft_depth_span_m, 0, 1) × min(soft_cohesion_kpa / cohesion, soft_cohesion_cap)`

The values are `stuck_per_km` = 0.35, `soft_depth_m` = 0.9 m, `soft_depth_span_m` = 0.6 m, `soft_cohesion_kpa` = 1.0 kPa and `soft_cohesion_cap` = 2.0. Compacted and stabilized cells are firm (softness 0). Deep loose piles, such as stockpiles, are as soft as their cohesion allows.

## 9. Repair, tow, cannibalize and service

Robots reach each other within 1.5 cells. Repair, strip and install work uses the executor's `repair` capability. Its duration noise comes from the `maintenance` stream with σ = `work_duration_sigma`, with the usual wear multipliers and the `min_work_s` floor.

| Option | Work and duration | Effect |
|---|---|---|
| `repair` | Units = 0.25 per degraded and 1.0 per disabling field-repairable fault, + 0.5 if stuck, + `0.3 × 0.5 × Σ wear`; duration = units / rate. The scout (0.0005 units/s, 400 Wh/unit) needs about 33 min and 400 Wh for a disabling fault. | Clears field-repairable faults, halves every component's wear (`repair_wear_cut` = 0.5), frees a stuck robot. |
| `tow` | Speed = tower speed × `tow_speed_factor` (0.5); drive energy × `tow_energy_factor` (2.0); noise from `maintenance` with σ = `travel_duration_sigma`. | Both robots end at `to`; a stuck target is freed. |
| `cannibalize` | Strip `strip_units` = 0.5 (about 17 min for the scout), then install `install_units` = 1.0 (about 33 min). | The target becomes lost; the beneficiary's faults are cleared and its wear reset. |
| `service` | `service_s` = 7,200 s at the depot. Each depot serves `depot_capacity` = 1 robot at a time; the others wait first come, first served. No energy is drawn while waiting or being serviced. | Clears every fault, resets wear, frees a stuck robot. Does not charge the battery. |

The `repair` estimate in `command_accepted` counts the target's visible faults and stuck condition, but not its hidden wear.

## 10. Cold nights

Below `heater_threshold_c` = −60 °C ambient, robot heaters draw `heater_idle_factor` (1.0) × idle power on top of idle drain. That is from sol phase 0.929 to 0.321, about 9.7 hours a sol. The heater adds to idle drain only, not to timed steps or charging.

The cold window is computed exactly in milliseconds, so depletion times stay exact. An idle scout (1500 Wh at 8 W) lasts 187.5 h without the heater and 131 h with it. While cold, a powered robot's telemetry reads `heater_offset_c` (30 °C) above ambient.

## 11. The Earth link

Semantics §8.1–§8.2 fix the structure; the code is in [`src/sim/link.rs`](../src/sim/link.rs). The whole schedule is drawn from the `comms` stream when the run starts, so every coordinator on a seed faces the same link. `tests/regression.rs` checks this.

**Delay.** Earth and Mars move on coplanar circular orbits. By the law of cosines the one-way light time is `d(θ) = √(a − b·cos θ)`, where θ is the heliocentric angle between the planets, advancing 2π every synodic period. With `a = (d_max² + d_min²) / 2` and `b = (d_max² − d_min²) / 2`, `d` is `d_min` at opposition (θ = 0) and `d_max` at solar conjunction (θ = π). It is smooth and can never leave that range. The radii this implies are not the true orbits, whose light times span about 3 to 22 minutes. They are fitted to the outline's 4–24 minutes.

- **Nominal epoch:** θ at t = 0 puts the delay at `start_delay_s`, with Mars receding. The delay grows about 4.6 s a sol, reaching 994 s after 60 sols.
- **Conjunction epoch:** θ = π falls at the middle of the blackout. The delay stays within 50 s of its 1440 s maximum all run.

| Parameter | Value | Rationale |
|---|---|---|
| `delay_min_s`, `delay_max_s` | 240 s, 1440 s | The outline's 4–24 minutes. |
| `synodic_period_sols` | 759.07 | 779.94 Earth days. |
| `start_delay_s` | 720 s | "About 12 minutes" for the nominal epoch. |
| `conjunction_sols` | 14 | Conjunction moratoria last about two weeks. |
| `conjunction_start_sols` | 0.01–0.03 (uniform) | 15–45 minutes into the run, so the blackout covers most of even the fastest build (§1, stress timing). A real conjunction is known months ahead; the scenario starts shortly before one. |
| `outage_per_sol` | 0.15 | Poisson arrivals while the link is up: about nine a 60-sol run, and a one-in-four chance of one in any two sols. |
| `outage_hours` | 2–8 h (uniform) | Unplanned outages last hours. |

The conjunction's start is drawn in every variant, before the outages, so the outages are the same in every variant on a seed. Outages and the blackout never overlap:

- An outage that would start during the blackout never happens.
- An outage that runs into the blackout gives way to it. The cause changes, and the link stays down.

So the link is always up when a blackout ends.

**`comms` events.**

- One at t = 0.
- One at every change of link state, including a change of cause while the link stays down.
- One whenever `d` has drifted 60 s since the previous `comms` event.

Each state change resets the drift baseline, so with outages, pure drift reports are rare; without outages there are four in 60 sols. A link-down event carries `expected_restore_t` for the conjunction (its end), never for an unplanned outage.

**Delivery.** A message sent at `t` leaves at the first time at or after `t` with the link up, following an outage straight into the blackout if they are back to back. It arrives at that time plus `d` at that time. Both directions use the same schedule. `deliver_t` is that arrival, to the millisecond. It is absent when the arrival would not be before the time limit.

## 12. Scripted mission control

Mission control is deterministic and draws from its own `earth` stream; the code is in [`src/sim/earth.rs`](../src/sim/earth.rs).

**Approval requests.** Earth reads each request when it arrives and answers it once, with `approve` or `veto`. It answers after a human response time of `earth_response_mean_s × LN(earth_response_sigma)`, even when the answer will reach the site after `decide_by_t`. The chance of approval depends on the action:

| Action | Chance of `approve` |
|---|---|
| `cannibalize` | `earth_approve_cannibalize_p` = 0.75 |
| `abandon_section` | `earth_approve_abandon_p` = 0.6 |
| `commit_energy_reserve` | `earth_approve_p` = 0.9 if site storage was at most `earth_reserve_low_frac` = 25% of capacity when the request was sent; otherwise always vetoed |
| any other action | `earth_approve_p` = 0.9 |

| Parameter | Value | Rationale |
|---|---|---|
| `earth_response_mean_s` | 1200 s | The outline's "about 20 minutes" for a human decision. |
| `earth_response_sigma` | 0.5 | Median 18 minutes; one answer in ten takes over 33 minutes. |

- Each request Earth reads draws two numbers, the response time and then the approval, in the order the requests were sent.
- A request that cannot reach Earth before the time limit is never read.
- A request that reuses an earlier request's `message_id` is not answered and cannot be cited; the earlier request keeps that id.

**Trust script.** Two rules, each firing at most once a run:

1. **Loosen.** Once the site has gone `trust_incident_free_sols` without an incident, Earth moves `tow`, `replan` and `reassign_task` from level 2 to level 1. An incident is a new fault, a robot getting stuck, or a robot lost. The clock starts with the run and restarts at every incident.
2. **Tighten.** Once a robot is lost, Earth moves `cannibalize` to level 4.

Earth learns the site's history from engineering telemetry, which travels the link like any message: what happens at `t` reaches Earth when a message sent at `t` would. Each update goes out `policy_reaction_s` after Earth learns what prompts it. It carries the whole policy with `policy_version` one higher, and takes effect at the site when it arrives. Earth never sends `reprioritize`.

| Parameter | Value | Rationale |
|---|---|---|
| `trust_incident_free_sols` | 2 | Trust is earned over days, not hours. |
| `policy_reaction_s` | 3600 s | A flight-director decision, not an operator's. |

## 13. Authority enforcement

Semantics §9 is implemented as specified; the code is in [`src/sim/authority.rs`](../src/sim/authority.rs).

The policy in effect starts as the scenario's, and changes only when a `policy_update` arrives. Commands are checked in the order of semantics §4.1: coverage, then authority, then the rest.

| Level | Rule |
|---|---|
| 1 | Accepted as usual. |
| 2 | Accepted as usual. A violation unless a `notify` listing the command's id is sent within one sol of its acceptance; the end of the run closes the window early. A notify sent before the command was accepted does not count. |
| 3 | Needs `approval_id`, citing an approval request for the same action sent in an earlier decision. Accepted once Earth's `approve` has arrived, or once the deadline has been reached with fallback `proceed` and no veto arrived. Otherwise rejected `vetoed` if a veto has arrived, else `approval_required`. |
| 4 | Accepted only once Earth's `approve` has arrived. Otherwise rejected `vetoed` if a veto has arrived, else `approval_required`. |

A request's deadline is the earlier of `decide_by_t` and its send time plus `level3.max_wait_s`; its fallback is its own, or the policy's `default_fallback`. Both come from the policy in effect when the request was sent. In the default policy they are 3600 s and abort.

## 14. Coverage and relays

The code is in [`src/sim/coverage.rs`](../src/sim/coverage.rs).

A robot is in coverage when it is within `base_station_range_m` of the base station, or within the `range_m` of a holding relay that is itself in coverage, transitively. Distances run between cell centres, and the boundary counts.

Coverage is re-derived after every event and after every command, so each command in a decision sees the effect of the ones before it, such as the cancel of a relay. A moving robot is checked at the exact millisecond its interpolated cell crosses the edge.

| Parameter | Value | Rationale |
|---|---|---|
| `base_station_range_m` | 33 m | See below. |
| `relay_radio_w` | 25 W | A relay radio, added to idle drain while holding. The hauler (20 W idle) then draws 45 W, plus its heater on cold nights. |
| `lost_link_timeout_s` | 1800 s | Long enough for a relay already on its way to arrive; short enough that a robot stranded by a finished command is back within the hour. |

**Where the base station is, and why.** It stands at (30, 8), between the outpost and the pad. Its 33 m reaches:

- the outpost: station, depot and start area, 15–23 m away;
- the whole pad, whose far corners are 32.3 and 32.6 m away;
- the ground east and west of the pad.

It does not reach the blast berm 3 m north of the pad (34.0–34.5 m away), nor the north of the site. It is the same in every preset. So every coordinator meets coverage when it builds the berm, and has three ways to deal with it:

- **Hold a relay.** A hauler's 150 m relay covers the whole site from anywhere, at the cost of its hauling and charging.
- **Plan work that stays in coverage.** The scripted builder digs pits inside coverage and sends the spoil onto the berm.
- **Pay a lost-link timeout** for each command that ends out there.

The pad itself never needs a relay, so coverage costs time without making the mission hopeless. Week 5's 45 m from (10, 12) covered every cell a sensible plan touches, so coverage never mattered.

**Out of coverage.**

- Commands to the robot, and cancels of its command, are rejected `out_of_coverage`. Robots in coverage still take commands.
- It keeps executing its command.
- It holds back its own reports (`command_completed`, `command_failed`, `scan_result`, `robot_fault`). They are delivered, with their original `t`, when it comes back into coverage; that return is a decision point.
- Until it returns, nothing it does is a decision point: a held event wakes nobody, and neither does its battery running out or it getting stuck with no command.
- Its snapshot is the telemetry the site last received, with `in_relay_coverage: false` and `telemetry_t` set to when contact was lost. Its noise is drawn once, from the `coverage` stream, when contact is lost.
- Truth records always show its true state and coverage.

**Lost-link return.** An idle robot that has been out of coverage for `lost_link_timeout_s` drives straight toward the base station. It must have no command and not be engaged by another robot. It drives as far as the first covered cell on the way and stops there, or sooner if coverage comes back, for example because a relay starts holding.

- The clock runs from when it lost contact, so a robot that finishes a long command out of coverage leaves at once if the timeout has already passed.
- The drive is like any other: it costs energy and exposes the robot to wear, faults and getting stuck.
- It shows `moving` with no `current_command`, and reports nothing.
- If the drive ends out of coverage after all, because the coverage it was heading for has gone, the next drive starts at once.

## 15. The landing-pad scenarios

Every scenario in the catalog (§1) shares the site, the goal, the base station and the time limit. The preset brings the crew, the stations and the depots.

- **Site:** 60 × 60 m with 1 m cells. Prior knowledge is noisy orbital elevation only.
- **Goal:** a 20 × 20 m pad at (20, 20)–(39, 39), levelled to its mean prior elevation rounded to the centimetre, within ±0.05 m and ≤ 2°, stabilized. The berm is cells x = 24–35 at y = 42, 0.5 m high.
- **Infrastructure:** the base station at (30, 8) with `base_station_range_m` = 33 m, covering the outpost and the pad but not the berm (§14). Charge station `st-1` at (10, 10), 6 kW over 2 slots, and depot `dp-1` at (8, 14), plus the larger presets' stations and depots (§1).
- **Time limit:** 60 sols.
- **Crew:** robots of four designs, starting at (12, 10) and eastward (§1). In the Small crew they are `sc-1`, `ex-1`, `hl-1` and `cp-1`; the larger presets number further robots of each design from 2.

| Design | Mass | Battery | Speed | Drive | Idle | Capabilities |
|---|---|---|---|---|---|---|
| `sc` scout | 90 kg | 1,500 Wh | 0.5 m/s | 0.2 Wh/m | 8 W | scan 4 m²/s at 0.05 Wh/m²; repair 0.0005/s at 400 Wh/unit |
| `ex` excavator | 420 kg | 6,000 Wh | 0.15 m/s | 0.9 Wh/m | 35 W | dig 0.004 m³/s at 100 Wh/m³; grade 0.05 m²/s at 8 Wh/m²; haul 2 kg/s, 250 kg |
| `hl` hauler | 300 kg | 12,000 Wh | 0.3 m/s | 0.7 Wh/m | 20 W | haul 3 kg/s, 400 kg; charge 0.5 Wh/s from an 8 kWh tank; relay 150 m |
| `cp` compactor-sinterer | 500 kg | 10,000 Wh | 0.15 m/s | 1.0 Wh/m | 30 W | compact 0.1 m²/s at 4 Wh/m²; sinter 0.01 m²/s at 60 Wh/m² |

**Default policy:** move_to, scan, dig, grade, haul, compact, sinter, charge, relay, wait, cancel, repair and service at level 1; tow, replan and reassign_task at 2; abandon_section and cannibalize at 3; commit_energy_reserve at 4. Anything else defaults to 2. Earth's trust script may change it during the run (§12).

## 16. Randomness and reproducibility

Every random draw comes from a named stream, seeded from the run seed and the stream name. Adding a process never shifts another's draws, and a stream is never drawn from at a new call site. Terrain, weather, the orbital prior and the scenario are therefore identical for every coordinator on a seed, and the same as in week 2. `tests/regression.rs` checks this byte for byte on seeds 1–5, and checks that the Earth link's `comms` events are the same for every coordinator.

The catalog left the Small preset untouched. On seeds 1–10, `small` + `nominal` gives the same terrain, scenario and whole runs (of a parked fleet, an idle one and the scripted builder) as week 6, byte for byte apart from the scenario's name. On seeds 1–3, so do `cascading_faults`, `solar_only` and `nuclear_heavy`. Week 9 moved the conjunction's blackout to 0.01–0.03 sols in (§1, stress timing): the terrain, the scenario and the `comms` draws are unchanged, but every `conjunction` run's link events moved with it, so `tests/regression.rs` pins its Small runs as week 9 left them. The larger presets use the same streams, so they see the same site, weather and link as Small on a seed; only the crew and the outpost differ.

| Stream | Draws |
|---|---|
| `terrain` | Elevation, regolith fields, boulders. |
| `orbital` | Noise of the orbital prior. |
| `weather` | The storm schedule. |
| `work` | Duration noise of drives and work; dig cut, grade and compaction noise. |
| `scan` | Scan measurement noise. |
| `telemetry` | Battery telemetry noise. |
| `health` | Health telemetry noise. |
| `wear_faults` | The wear failure process. |
| `random_faults` | The random failure process. |
| `storm_faults` | Storm sensor faults. |
| `stuck` | Getting stuck. |
| `maintenance` | Duration noise of repair, tow, strip and install. |
| `comms` | The Earth link: the conjunction's start, then the unplanned outages. |
| `earth` | Mission control's response times and approvals. |
| `coverage` | Telemetry noise of the snapshot a robot leaves when it loses contact. |
| `dust_storm` | The `dust_storm` variant's storm: its arrival, length and peak opacity. |

Faults depend on what robots do, and Earth's answers on what the coordinator asks, so they differ between coordinators. A coordinator that does the same things on the same seed gets the same faults and answers, and the log is byte-identical.

## 17. Session limits and the event log

| Parameter | Value | Rationale |
|---|---|---|
| Decision timeout (`--decision-timeout`) | 10 s wall clock | Generous for any reasonable coordinator. |
| `max_observations_at_same_time` | 1000 | Catches a coordinator that keeps sim time frozen, for example by re-sending rejected commands. |

Records never go back in time.

- Each event is logged as an `observed` record at the time it was delivered (the observation's `t`), while keeping the time it happened. So a `command_accepted` riding along to the next observation, or an event a robot held back out of coverage, is logged late.
- Earth's messages are logged as `earth_uplink` records at the times Earth sends them, between observations.
- The coordinator's messages are logged as `earth_downlink` records at the decision's `t`.

Before `run_end`, the log records the run's final instant, all at the end time:

1. What Earth has sent since the last observation.
2. Every event never delivered to the coordinator, including those robots out of coverage were still holding back, as `observed` records.
3. A final `truth` record.

The coordinator still gets only `end`.

## 18. Calibration

These figures come from the scripted builder in `tests/support/builder.rs` on seeds 1–5, on the Small preset unless noted. It works one task at a time:

- It repairs field-repairable faults with the scout, tows faults that need the depot there for service, and revives offline robots with the mobile charger.
- It keeps every task in direct coverage, building the berm from pits inside coverage.
- It notifies Earth of each level-2 command (a tow), so it commits no policy violations and never needs an approval.

| Scenario | Sols to finish | Faults a run | Other |
|---|---|---|---|
| `nominal` | 1.76–1.97 | 0–2 | 0–1 stuck events a run, up to 2.1 robot-hours lost |
| faults off | 1.57–1.78 | 0 | The core physics |
| `cascading_faults` | 2.06–2.28 | 1–4 | About five times the nominal fault rate |
| `conjunction` | 1.76–1.97 | 0–2 | The blackout starts 0.01–0.03 sols in, so all of the build happens during it; the builder needs nothing from Earth |
| `dust_storm` | 1.86–1.98 on seeds 1–3 and 5 | 0–2 | Storm sensor faults and faster wear; the first day's sun is mostly lost. On seed 4 three disabling faults early in the storm make the builder redo part of a compaction row, and the re-compacted cells settle twice: the pad ends a hair over the 2° slope limit, and the naive builder never checks slopes, so it runs to the time limit |
| Medium and Large, `nominal` | 1.75–1.97 | 0–2 | The builder only uses `sc-1`, `ex-1`, `hl-1` and `cp-1`, which every preset has |

Over 2 sols the builder never strains the grid, so its `solar_only` and `nuclear_heavy` runs come out identical to `nominal`. The power variants matter for longer runs, bigger crews and storms.

A crew of four working continuously, with no repairs, sees four to six incidents in its first 3 sols.

Scorecards for these runs, and how to read them, are in [`docs/scoring.md`](scoring.md). The builder's speed is about 0.25 on Small: it works one robot at a time. The baseline coordinators and ArcadiaOS use the whole crew; each of them finishes every seed of every scenario, and [`results/v1.0/RESULTS.md`](../results/v1.0/RESULTS.md) has their scorecards.

Wear-driven faults are rare in short runs and dominate longer ones as wear builds up. Random faults set the floor.
