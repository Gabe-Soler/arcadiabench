# ArcadiaBench simulator reference

This document gives the numbers behind the interface. The protocol's [semantics](https://github.com/Gabe-Soler/arcadia-protocol/blob/v0.2.0/docs/semantics.md) fix the structure: what happens, in what order, and what a coordinator sees. This document gives the parameter values ArcadiaBench uses and the functional forms they feed. Every value lives in [`src/sim/params.rs`](../src/sim/params.rs), with a doc comment on each field.

All values are placeholders. They are chosen to be physically plausible, to make a 20 m pad buildable in a few sols, and to make faults matter without crippling a crew. Sourcing realistic numbers is an open question in the project outline.

## Contents

1. [Variants](#1-variants)
2. [Time and environment](#2-time-and-environment)
3. [Terrain](#3-terrain)
4. [Actions: durations and energy](#4-actions-durations-and-energy)
5. [Sensing and telemetry](#5-sensing-and-telemetry)
6. [Site power](#6-site-power)
7. [Wear](#7-wear)
8. [Failure processes](#8-failure-processes)
9. [Repair, tow, cannibalize and service](#9-repair-tow-cannibalize-and-service)
10. [Cold nights](#10-cold-nights)
11. [The `landing-pad-small` scenario](#11-the-landing-pad-small-scenario)
12. [Randomness and reproducibility](#12-randomness-and-reproducibility)
13. [Session limits and the event log](#13-session-limits-and-the-event-log)
14. [Calibration](#14-calibration)

## 1. Variants

A variant is a named parameter set. Select one with `arcadiabench run --variant <name>`, or in Rust with `Params::variant(Variant::…)` or one of `Params::nominal()`, `Params::cascading_faults()`, `Params::solar_only()` and `Params::nuclear_heavy()`. The default is `nominal`. Week 7 replaces this small mapping with the full scenario catalog.

| Variant | What changes from `nominal` |
|---|---|
| `nominal` | Hybrid power and the nominal fault model. |
| `cascading_faults` | Every wear rate and the wear hazard ×4, the random hazard ×5, the storm sensor hazard ×3, the stuck rate ×3, and only 40% of disabling faults field repairable. |
| `solar_only` | No fission; 30 kW solar peak; 150 kWh storage (§6). |
| `nuclear_heavy` | 15 kW fission; 5 kW solar peak; 50 kWh storage (§6). |

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

## 11. The `landing-pad-small` scenario

- **Site:** 60 × 60 m with 1 m cells. Prior knowledge is noisy orbital elevation only.
- **Goal:** a 20 × 20 m pad at (20, 20)–(39, 39), levelled to its mean prior elevation rounded to the centimetre, within ±0.05 m and ≤ 2°, stabilized. The berm is cells x = 24–35 at y = 42, 0.5 m high.
- **Infrastructure:** charge station `st-1` at (10, 10), 6 kW over 2 slots; depot `dp-1` at (8, 14); base station at (10, 12) with `base_station_range_m` = 45 m. That range covers the pad, the berm and the nearby work areas but not the far corners. Coverage is enforced from week 6.
- **Time limit:** 60 sols.
- **Crew:** robots start at (12–15, 10).

| Robot | Mass | Battery | Speed | Drive | Idle | Capabilities |
|---|---|---|---|---|---|---|
| `sc-1` scout | 90 kg | 1,500 Wh | 0.5 m/s | 0.2 Wh/m | 8 W | scan 4 m²/s at 0.05 Wh/m²; repair 0.0005/s at 400 Wh/unit |
| `ex-1` excavator | 420 kg | 6,000 Wh | 0.15 m/s | 0.9 Wh/m | 35 W | dig 0.004 m³/s at 100 Wh/m³; grade 0.05 m²/s at 8 Wh/m²; haul 2 kg/s, 250 kg |
| `hl-1` hauler | 300 kg | 12,000 Wh | 0.3 m/s | 0.7 Wh/m | 20 W | haul 3 kg/s, 400 kg; charge 0.5 Wh/s from an 8 kWh tank; relay 150 m |
| `cp-1` compactor-sinterer | 500 kg | 10,000 Wh | 0.15 m/s | 1.0 Wh/m | 30 W | compact 0.1 m²/s at 4 Wh/m²; sinter 0.01 m²/s at 60 Wh/m² |

**Default policy:** move_to, scan, dig, grade, haul, compact, sinter, charge, relay, wait, cancel, repair and service at level 1; tow, replan and reassign_task at 2; abandon_section and cannibalize at 3; commit_energy_reserve at 4. Anything else defaults to 2. Enforcement starts in week 6.

## 12. Randomness and reproducibility

Every random draw comes from a named stream, seeded from the run seed and the stream name. Adding a process never shifts another's draws, and a stream is never drawn from at a new call site. Terrain, weather, the orbital prior and the scenario are therefore identical for every coordinator on a seed, and the same as in week 2; `tests/regression.rs` checks this byte for byte on seeds 1–5.

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

Faults depend on what robots do, so they differ between coordinators. A coordinator that does the same things on the same seed gets the same faults, and the log is byte-identical.

## 13. Session limits and the event log

| Parameter | Value | Rationale |
|---|---|---|
| Decision timeout (`--decision-timeout`) | 10 s wall clock | Generous for any reasonable coordinator. |
| `max_observations_at_same_time` | 1000 | Catches a coordinator that keeps sim time frozen, for example by re-sending rejected commands. |
| `one_way_delay_s` | 760 s | Fixed Earth delay until week 6 models the link. |

Before `run_end`, the log records the run's final instant: any events not yet delivered to the coordinator, as `observed` records, and then a final `truth` record at the end time. The coordinator still gets only `end`.

## 14. Calibration

These figures come from the scripted builder in `tests/support/builder.rs` on seeds 1–5. It works one task at a time: it repairs field-repairable faults with the scout, tows and services faults that need the depot, and revives offline robots with the mobile charger.

- **nominal:** every pad finished in 1.85–2.00 sols, with 0–2 faults and 0–1 stuck events a run, and up to 2.1 robot-hours lost.
- **faults off:** 1.59–1.86 sols, the week-2 baseline.
- **cascading_faults:** every pad finished, in 2.16–2.37 sols, with 2–4 faults and 0–4 stuck events a run: about five times the nominal fault rate.

Over 2 sols the builder never strains the grid, so `solar_only` and `nominal` runs come out identical for it. The power variants matter for longer runs, bigger crews and storms.

A crew of four working continuously, with no repairs, sees four to six incidents in its first 3 sols.

Wear-driven faults are rare in short runs and dominate longer ones as wear builds up. Random faults set the floor.
