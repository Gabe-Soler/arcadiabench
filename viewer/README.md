# ArcadiaBench replay viewer

A static web page that replays an ArcadiaBench run from its event log: a top-down view of the site with the robots at work, the landing pad taking shape, site power through dust storms, and the Earth link through outages and blackouts.

It is plain HTML, CSS and ES modules drawn on Canvas 2D, with no build step and no framework. It follows the system's light or dark setting, switching live when the setting changes. Its fonts, Inter and Manrope, come from Google Fonts when the network allows; without it the page uses system fonts, so it still works offline from any static file server.

## Use

Serve the repository with any static server, from its root, and open the page:

```sh
python3 -m http.server 8000
# then open http://localhost:8000/viewer/
# or a demo replay: http://localhost:8000/viewer/?log=../results/v1.0/replays/landing-pad-large-nominal-1.jsonl.gz
```

Then load a log in any of three ways:

- **Open log** in the top bar, or **Choose a log file** on the start screen.
- Drag a file anywhere onto the page.
- `?log=` in the address, for example `http://localhost:8000/viewer/?log=samples/arcadiaos-conjunction-seed13.jsonl.gz`.

Logs can be `.jsonl` or gzip-compressed `.jsonl.gz`. Compression is detected from the file's contents, not its name, so a server that decompresses `.gz` files on the fly works too. Opening the page straight from `file://` does not work, because browsers block ES modules there. The page says so if it happens.

There are logs to try in two places:

- The benchmark's four demo replays, ArcadiaOS 1.0 runs on protocol 1.0, are in [`results/v1.0/replays/`](../results/v1.0/replays/); [`docs/demos.md`](../docs/demos.md) describes them and the moments worth a look.
- Four sample logs ship in [`samples/`](samples/README.md), so older logs stay covered: three ArcadiaOS and null-coordinator runs on protocol 0.2, and one protocol 0.1 run.

### URL parameters

| Parameter | Meaning |
|---|---|
| `log=URL` | Log to open, relative to the page or absolute. A log on another site must allow cross-origin requests. |
| `t=TIME` | Start at this sim time. Plain seconds (`t=60000`), or with a unit: `s`, `m`, `h` or `sol` (`t=1.5sol`). |
| `robot=ID` | Select a robot and show its details. |
| `tab=events\|fleet\|run` | Sidebar tab to show. |
| `speed=N` | Playback speed in sim seconds per real second. |
| `play=1` | Start playing at once. |

**Copy link** copies the current moment as a link. While paused, the address bar also keeps `t` up to date, so it can be copied directly. Every demo is a shareable link. A log opened from a local file cannot be linked, so its link carries only the time.

### Controls

| Input | Action |
|---|---|
| <kbd>Space</kbd> | Play or pause |
| <kbd>←</kbd> <kbd>→</kbd> | Previous or next decision point |
| <kbd>Shift</kbd> + <kbd>←</kbd> <kbd>→</kbd> | Back or forward 1% of the run |
| <kbd>Home</kbd> <kbd>End</kbd> | Start or end |
| <kbd>[</kbd> <kbd>]</kbd> | Slower or faster |
| <kbd>F</kbd> or double-click | Fit the site to the view |
| Wheel and drag | Zoom and pan |
| Click a robot | Details in the Fleet tab |
| Click a feed item | Jump to it |

## What it shows

- **Site:** hill-shaded terrain from `run_start.true_site`, updated by each truth record's `changed_cells`; surface states (natural, cleared, graded, compacted, stabilized, berm, stockpile) with a legend; boulders; the pad outline and berm target cells from the goal in `earth_message` events or `earth_uplink` records; charge stations, depots, and the base station with its coverage radius (protocol 0.2).
- **Robots:** a marker coloured by status, with the main capability as a glyph, a label, battery and payload bars, a fault badge (amber for degraded, red for disabled), a wrench badge while under repair or depot service, and a marker when out of relay coverage. A towed robot rides along with its tower, and dashed links join robots repairing, towing or stripping another. Active relays draw their coverage. Hover a robot for a summary, or click it for details (faults, telemetry age) and the path ahead of it. With large crews, labels that would cover another robot are left out; the selected or hovered robot is always labelled.
- **Time:** the sol clock and day/night (daylight runs from 25% to 75% of each sol, and sol length comes from the scenario). The timeline marks faults, failures, lost robots, dust storms, link-down periods and the run end.
- **Landing pad:** cells in spec and verified, how many cells have been worked on, and the cells in each surface state. The tag reads "in progress" as soon as work starts, even before any cell is in spec.
- **Gauges:** site storage, solar, fission and demand with a history chart, in every power mode; optical depth, temperature and storm state; Earth link state, delay, cause and expected restore, with the messages in flight between Mars and Earth and how many wait for the link. At night the map darkens, a dust storm washes it out, and while the link is down it goes dark.
- **Feed:** commands, completions, rejections, failures, faults, weather and link changes, and Earth messages in both directions (notifies, approval requests, status reports, approvals, vetoes and policy updates, with what each update changes and when each message arrives), with filters by kind and for the selected robot. Events a robot reported late, after being out of coverage, are flagged.
- **Fleet:** the selected robot's details, a count of robots per status, and every robot's status, battery and command.
- **Run:** from protocol 0.2, the scorecard from `run_end.metrics`, grouped as the benchmark scores it (completion, quality, energy, fleet health, autonomy, conditions), then the run, the pad now, activity counts and notes on the log.

### How robots move between snapshots

Truth records exist only at decision points, so a hauler can drive to the stockpile and back between two of them. Straight-line interpolation would show it gliding across the pad, so the viewer lays out each command's own path between snapshots instead: a haul's source and destination, an area's cells in sweep order, a station or a depot. The robot drives the path at its top speed and works at its waypoints for the rest of the time. Truth positions are always met exactly. Cell changes are timed to when the robot finishes each cell, so the pad fills in behind it. Commands the viewer does not know fall back to straight lines.

## Log versions

| Protocol | Support |
|---|---|
| 0.1 | Full. Power and weather come from `power` and `weather` events, and comms from `comms` events. |
| 0.2 | Full, adding `truth.site` (preferred for power, weather and link), `run_end.metrics`, robot `faults` and `telemetry_t`, the `lost` status, `scenario.power` and `scenario.base_station`, and the `service` command. |
| 1.0 | Full. Protocol 1.0 has 0.2's messages unchanged, so everything above applies. |
| Newer | Loads with a notice. Unknown record kinds, event types, command types, statuses, surface states and fields are ignored or shown generically. |

Bad lines are skipped and reported with their line numbers. A log whose first record is not `run_start` is rejected with an explanation.

Simulators from protocol 0.2 on write the run's final instant: the events still pending and a last `truth` record at `run_end`'s time. The viewer uses them as they are, and does not count that last record as a decision. Older logs end at the last decision, so there the viewer infers the end: commands issued at the last decision run to the end, and on `goals_complete` the closing verification scan counts as finished.

Logs up to tens of megabytes load in a few seconds. Parsing streams the file and yields to the page as it goes, and every per-frame query is a binary search over precomputed indexes.

## Code

| Path | Contents |
|---|---|
| `src/parse.js` | Fetching, gzip detection, JSONL streaming |
| `src/model.js` | The replay model: indexes and state at time t |
| `src/motion.js` | Robot motion between snapshots |
| `src/site.js` | Cell timelines and pad progress |
| `src/series.js` | Time-series search and interpolation |
| `src/feed.js` | Event feed items, Earth message texts and policy changes |
| `src/scorecard.js` | The run metrics grouped as the scorecard |
| `src/layout.js` | Spreading robots and placing labels on crowded maps |
| `src/format.js` | Formatting helpers |
| `src/render/` | Canvas drawing: terrain, map, timeline, chart, glyphs, palette |
| `src/ui/panels.js` | Sidebar panels and legend |
| `src/app.js` | Page wiring and the render loop |

Everything outside `src/render/`, `src/ui/` and `src/app.js` is free of DOM dependencies and is covered by the tests.

## Test

With Node 24 and no dependencies, from the repository root:

```sh
node --test viewer/test/
```

The tests cover parsing (including gzip, chunk boundaries and bad input), the motion model, pad progress and its stage, the model on inline logs (protocol 0.1, the final instant, a 20-robot crew, and the version notice, which protocol 1.0 does not get), the scorecard, the feed's Earth messages, label layout for 20 robots, the hand-made 0.2 fixture in `test/fixtures/` (faults, a lost robot, a link outage, late events, `truth.site` and metrics), all four samples, and every demo replay in `results/v1.0/replays/`, each replayed from start to end. CI runs them on every push and pull request (`.github/workflows/viewer.yml`).

## Deploy

The page is static, so publishing `viewer/` to any static host works, with logs alongside it or anywhere that allows cross-origin requests.

For GitHub Pages, run the **Viewer Pages** workflow (`.github/workflows/pages.yml`) by hand from the Actions tab; it never runs on push. The repository's Pages source must be set to GitHub Actions. The workflow runs the tests, then publishes a site that keeps the repository's paths:

- `viewer/` (without its tests), with the samples;
- `results/`, when the repository has it, such as the replays in `results/v1.0/replays/`;
- a root page that forwards to `viewer/`.

A replay under `results/` opens with a path relative to the viewer, so demo links look like:

```text
https://<owner>.github.io/<repo>/viewer/?log=../results/v1.0/replays/<run>.jsonl.gz&t=60000
https://<owner>.github.io/<repo>/viewer/?log=samples/arcadiaos-conjunction-seed13.jsonl.gz&t=68300
```

The same links work locally from `python3 -m http.server` run at the repository root.
