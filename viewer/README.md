# ArcadiaBench replay viewer

A static web page that replays an ArcadiaBench run from its event log: a top-down view of the site with the robots at work, the landing pad taking shape, site power through dust storms, and the Earth link through outages and blackouts.

It is plain HTML, CSS and ES modules drawn on Canvas 2D. There is no build step, no framework and no network dependency, so it works offline from any static file server.

## Use

Serve the repository (or just this folder) with any static server and open the page:

```sh
python3 -m http.server 8000
# then open http://localhost:8000/viewer/
```

Then load a log in any of three ways:

- **Open log…** in the top bar, or **Choose a file…** on the start screen.
- Drag a file anywhere onto the page.
- `?log=` in the address, for example `http://localhost:8000/viewer/?log=samples/builder-seed34.jsonl.gz`.

Logs can be `.jsonl` or gzip-compressed `.jsonl.gz`. Compression is detected from the file's contents, not its name, so a server that decompresses `.gz` files on the fly works too. Opening the page straight from `file://` does not work, because browsers block ES modules there. The page says so if it happens.

Two sample logs ship in [`samples/`](samples/README.md).

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
- **Robots:** a marker coloured by status, with the main capability as a glyph, a label, battery and payload bars, a fault badge (amber for degraded, red for disabled), and a marker when out of relay coverage. Active relays draw their coverage. Hover a robot for a summary, or click it for details and the path ahead of it.
- **Time:** the sol clock and day/night (daylight runs from 25% to 75% of each sol, and sol length comes from the scenario). The timeline marks faults, failures, lost robots, dust storms, link-down periods and the run end.
- **Gauges:** site storage, solar, fission and demand with a history chart; optical depth, temperature and storm state; Earth link state, delay, cause and expected restore, with the messages in flight between Mars and Earth. At night the map darkens, a dust storm washes it out, and while the link is down it goes dark.
- **Feed and summary:** commands, completions, rejections, failures, faults, weather and link changes, and Earth messages in both directions, with filters. Events delivered late (from robots out of coverage) are flagged. The Run tab summarises the run: coordinator, seed, end reason, sols, pad progress, activity counts and, from protocol 0.2, the run metrics.

### How robots move between snapshots

Truth records exist only at decision points, so a hauler can drive to the stockpile and back between two of them. Straight-line interpolation would show it gliding across the pad, so the viewer lays out each command's own path between snapshots instead: a haul's source and destination, an area's cells in sweep order, a station or a depot. The robot drives the path at its top speed and works at its waypoints for the rest of the time. Truth positions are always met exactly. Cell changes are timed to when the robot finishes each cell, so the pad fills in behind it. Commands the viewer does not know fall back to straight lines.

## Log versions

| Protocol | Support |
|---|---|
| 0.1 | Full. Power and weather come from `power` and `weather` events, and comms from `comms` events. |
| 0.2 | Full, adding `truth.site` (preferred for power, weather and link), `run_end.metrics`, robot `faults` and `telemetry_t`, the `lost` status, `scenario.power` and `scenario.base_station`, and the `service` command. |
| Newer | Loads with a notice. Unknown record kinds, event types, command types, statuses, surface states and fields are ignored or shown generically. |

Bad lines are skipped and reported with their line numbers. A log whose first record is not `run_start` is rejected with an explanation.

Logs up to tens of megabytes load in a few seconds. Parsing streams the file and yields to the page as it goes, and every per-frame query is a binary search over precomputed indexes.

## Code

| Path | Contents |
|---|---|
| `src/parse.js` | Fetching, gzip detection, JSONL streaming |
| `src/model.js` | The replay model: indexes and state at time t |
| `src/motion.js` | Robot motion between snapshots |
| `src/site.js` | Cell timelines and pad progress |
| `src/series.js` | Time-series search and interpolation |
| `src/feed.js` | Event feed items |
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

The tests cover parsing (including gzip, chunk boundaries and bad input), the motion model, pad progress, the model on inline 0.1 logs, the hand-made 0.2 fixture in `test/fixtures/` (faults, a lost robot, a link outage, late events, `truth.site` and metrics), and both samples. CI runs them on every push and pull request (`.github/workflows/viewer.yml`).

## Deploy

The page is static, so publishing `viewer/` to any static host works, with logs alongside it or anywhere that allows cross-origin requests. For GitHub Pages, run the **Viewer Pages** workflow (`.github/workflows/pages.yml`) by hand from the Actions tab. It publishes `viewer/` as the site root, so a demo link looks like `https://<owner>.github.io/<repo>/?log=samples/builder-seed34.jsonl.gz&t=60000`. The repository's Pages source must be set to GitHub Actions.
