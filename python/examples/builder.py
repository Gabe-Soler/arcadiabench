"""A simple coordinator that builds the landing pad, one task at a time.

Run it in process, through the Python bindings::

    python examples/builder.py --seed 1 --preset small --variant nominal

or as a subprocess of the command line, speaking newline-delimited JSON::

    arcadiabench run --seed 1 -- python examples/builder.py --serve

The plan, after the scripted builder in ArcadiaBench's tests: survey the pad;
dig every row down to the target level, with the spoil sent to a stockpile;
dig a borrow pit; rescan; haul fill from the stockpile into low cells; grade,
compact and sinter every row; build the berm from pits dug inside coverage,
with their spoil sent onto it; then scan the pad again to verify it.

Robots are picked by capability, so it runs on every preset, using one robot
for each job. A robot low on charge charges first. Disabled robots are
rescued before any other work: field repair for what the field can fix,
towing to the depot for service for what it cannot, and the mobile charger
for flat batteries. Each level-2 command gets a notify to Earth. It is
deliberately naive: it never works two robots at once.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from collections import deque
from typing import Any, Deque, Dict, List, Optional, Set, Tuple

import arcadiabench

Message = Dict[str, Any]
Cell = Tuple[int, int]

STOCKPILE = {"x": 45, "y": 30}
BORROW = {"min": {"x": 44, "y": 20}, "max": {"x": 47, "y": 23}}
#: Cells lower than the target by more than this get fill.
FILL_THRESHOLD_M = 0.08
#: Berm material comes from pits this far south of each berm cell...
BERM_PIT_OFFSET = 2
#: ...dug this far below their prior elevation.
BERM_PIT_DEPTH_M = 1.6
ASSUMED_DENSITY_KG_M3 = 1550.0
LOW_BATTERY = 0.35
#: After a rejection, wait this long before trying again, s.
BACKOFF_S = 600.0


def rows(area: Message) -> List[Message]:
    """One rectangle per row of an area."""
    lo, hi = area["min"], area["max"]
    return [
        {"min": {"x": lo["x"], "y": y}, "max": {"x": hi["x"], "y": y}}
        for y in range(lo["y"], hi["y"] + 1)
    ]


def cells(area: Message) -> List[Cell]:
    lo, hi = area["min"], area["max"]
    return [(x, y) for y in range(lo["y"], hi["y"] + 1) for x in range(lo["x"], hi["x"] + 1)]


class Task:
    def __init__(self, robot: str, action: Message, rescue: bool = False) -> None:
        self.robot = robot
        self.action = action
        #: A rescue is not retried if it fails: the next one is worked out afresh.
        self.rescue = rescue


class Builder(arcadiabench.Coordinator):
    name = "python-builder"
    version = "1.0.0"

    def welcome(self, hello: Message) -> Message:
        scenario = hello["scenario"]
        robots = scenario["robots"]

        def having(capability: str) -> List[Message]:
            return [r for r in robots if any(c["name"] == capability for c in r["capabilities"])]

        def capacity(robot: Message, capability: str) -> float:
            return next(c.get("capacity") or 0.0 for c in robot["capabilities"] if c["name"] == capability)

        self.scanner = having("scan")[0]["robot_id"]
        self.repairer = having("repair")[0]["robot_id"]
        self.digger = having("dig")[0]["robot_id"]
        self.grader = having("grade")[0]["robot_id"]
        self.compactor = having("compact")[0]["robot_id"]
        self.sinterer = having("sinter")[0]["robot_id"]
        haulers = sorted(having("haul"), key=lambda r: -capacity(r, "haul"))
        self.hauler = haulers[0]["robot_id"]
        self.haul_kg = capacity(haulers[0], "haul")
        self.towers = [r["robot_id"] for r in haulers]
        self.charger = having("charge")[0]["robot_id"]
        workers = [self.scanner, self.repairer, self.hauler, self.charger, self.digger,
                   self.grader, self.compactor, self.sinterer]
        #: Rescued in this order: helpers first.
        self.crew = list(dict.fromkeys(workers))
        self.capacity = {r["robot_id"]: r["battery_capacity_wh"] for r in robots}
        self.station = scenario["stations"][0]
        self.depot = scenario["depots"][0]
        self.policy = scenario["policy"]
        self.prior = {
            (c["at"]["x"], c["at"]["y"]): c["cell"]["elevation_m"] for c in scenario["site"]["cells"]
        }
        self.pad: Optional[Message] = None
        self.phase = "start"
        self.tasks: Deque[Task] = deque()
        self.active: Optional[Tuple[str, Task]] = None
        self.robots: Dict[str, Message] = {}
        self.scanned: Dict[Cell, Message] = {}
        self.charger_tried: Set[str] = set()
        self.backoff_until = 0.0
        self.next_id = 0
        return super().welcome(hello)

    # ----- the plan -----

    def queue(self, robot: str, action: Message) -> None:
        self.tasks.append(Task(robot, action))

    def advance_phase(self) -> None:
        """Queue the next phase's tasks once the current phase is drained."""
        pad = self.pad
        assert pad is not None
        target = pad["target_elevation_m"]
        if self.phase == "start":
            self.queue(self.scanner, {"type": "scan", "area": pad["area"]})
            self.phase = "survey"
        elif self.phase == "survey":
            for row in rows(pad["area"]):
                self.queue(self.digger, {"type": "dig", "area": row, "target_elevation_m": target,
                                         "spoil_to": STOCKPILE})
            lowest = min(self.prior[c] for c in cells(BORROW))
            for row in rows(BORROW):
                self.queue(self.digger, {"type": "dig", "area": row,
                                         "target_elevation_m": lowest - 0.6, "spoil_to": STOCKPILE})
            self.phase = "dig"
        elif self.phase == "dig":
            self.queue(self.scanner, {"type": "scan", "area": pad["area"]})
            self.phase = "rescan"
        elif self.phase == "rescan":
            for c in cells(pad["area"]):
                cell = self.scanned.get(c, {})
                elevation = cell.get("elevation_m")
                if elevation is None or elevation >= target - FILL_THRESHOLD_M:
                    continue
                density = (cell.get("regolith") or {}).get("bulk_density_kg_m3") or ASSUMED_DENSITY_KG_M3
                left = (target - elevation) * density
                while left > 1.0:
                    chunk = min(left, self.haul_kg)
                    self.queue(self.hauler, {"type": "haul", "from": STOCKPILE,
                                             "to": {"x": c[0], "y": c[1]}, "mass_kg": chunk})
                    left -= chunk
            self.phase = "fill"
        elif self.phase == "fill":
            for row in rows(pad["area"]):
                self.queue(self.grader, {"type": "grade", "area": row, "target_elevation_m": target})
            for row in rows(pad["area"]):
                self.queue(self.compactor, {"type": "compact", "area": row})
            for row in rows(pad["area"]):
                self.queue(self.sinterer, {"type": "sinter", "area": row})
            self.phase = "finish"
        elif self.phase == "finish":
            # The berm is out of coverage, so it is built from pits inside
            # coverage, with the spoil sent onto it.
            for b in (pad.get("berm") or {}).get("cells", []):
                pit = {"x": b["x"], "y": b["y"] - BERM_PIT_OFFSET}
                self.queue(self.digger, {"type": "dig", "area": {"min": pit, "max": pit},
                                         "target_elevation_m": self.prior[(pit["x"], pit["y"])] - BERM_PIT_DEPTH_M,
                                         "spoil_to": b})
            self.phase = "berm"
        elif self.phase == "berm":
            self.queue(self.scanner, {"type": "scan", "area": pad["area"]})
            self.phase = "verify"
        else:
            self.phase = "done"

    # ----- robots -----

    def available(self, robot: str) -> bool:
        """Free to take a command: idle, not running one, and in coverage."""
        s = self.robots.get(robot)
        return (s is not None and s["status"] == "idle" and s.get("current_command") is None
                and s["in_relay_coverage"])

    def battery_low(self, robot: str) -> bool:
        s = self.robots.get(robot)
        return s is not None and s["battery_wh"] < LOW_BATTERY * self.capacity[robot]

    def charge(self, robot: str, source: Optional[str] = None, rescue: bool = False) -> Task:
        return Task(robot, {"type": "charge", "source": source or self.station["station_id"]}, rescue)

    def tower_for(self, robot: str) -> Optional[str]:
        return next((t for t in self.towers if t != robot and self.available(t)), None)

    def rescue_task(self) -> Optional[Task]:
        """The next step of rescuing a disabled robot, helpers first."""
        for robot in self.crew:
            s = self.robots.get(robot)
            # Out of coverage its snapshot is stale: nothing to act on yet.
            if s is None or not s["in_relay_coverage"]:
                continue
            here, status = s["position"], s["status"]
            if status == "offline":
                charger = self.robots.get(self.charger)
                if (robot != self.charger and self.available(self.charger)
                        and robot not in self.charger_tried and charger is not None):
                    at = charger["position"]
                    if math.hypot(at["x"] - here["x"], at["y"] - here["y"]) <= 1.5:
                        self.charger_tried.add(robot)
                        return self.charge(robot, self.charger, rescue=True)
                    return Task(self.charger, {"type": "move_to", "target": here}, rescue=True)
                if here == self.station["position"]:
                    return self.charge(robot, rescue=True)
                tower = self.tower_for(robot)
                if tower:
                    return Task(tower, {"type": "tow", "target_robot": robot,
                                        "to": self.station["position"]}, rescue=True)
            elif status == "stuck":
                if robot != self.repairer and self.available(self.repairer):
                    return Task(self.repairer, {"type": "repair", "target_robot": robot}, rescue=True)
                tower = self.tower_for(robot)
                if tower:
                    return Task(tower, {"type": "tow", "target_robot": robot,
                                        "to": self.depot["position"]}, rescue=True)
            elif status == "failed":
                repairable = all(f["field_repairable"] for f in s["faults"] if f["severity"] == "disabled")
                if repairable and robot != self.repairer and self.available(self.repairer):
                    return Task(self.repairer, {"type": "repair", "target_robot": robot}, rescue=True)
                if here == self.depot["position"]:
                    return Task(robot, {"type": "service", "depot": self.depot["depot_id"]}, rescue=True)
                tower = self.tower_for(robot)
                if tower:
                    return Task(tower, {"type": "tow", "target_robot": robot,
                                        "to": self.depot["position"]}, rescue=True)
        return None

    # ----- talking to the site -----

    def issue(self, task: Task, decision: Message) -> None:
        """Command a task, with the notify a level-2 action needs."""
        self.next_id += 1
        command_id = f"py-{self.next_id:05d}"
        self.active = (command_id, task)
        action = task.action["type"]
        level = self.policy["levels"].get(action, self.policy["default_level"])
        if level == 2:
            decision["to_earth"].append({
                "message_id": f"{command_id}-notify", "type": "notify", "action": action,
                "summary": f"{task.robot} {action}", "commands": [command_id],
            })
        decision["commands"].append({"command_id": command_id, "robot_id": task.robot, **task.action})

    def handle(self, event: Message) -> None:
        kind = event["type"]
        active_id = self.active[0] if self.active else None
        if kind == "earth_message":
            body = event["message"]
            if body["type"] == "goal" and body["objective"]["kind"] == "landing_pad":
                self.pad = body["objective"]["pad"]
            elif body["type"] == "policy_update":
                self.policy = body["policy"]
        elif kind == "scan_result":
            for c in event["cells"]:
                self.scanned[(c["at"]["x"], c["at"]["y"])] = c["cell"]
        elif kind == "command_completed" and event["command_id"] == active_id:
            self.active = None
        elif kind == "command_failed" and event["command_id"] == active_id:
            assert self.active is not None
            _, task = self.active
            self.active = None
            if task.rescue:
                return
            if event["reason"] == "insufficient_energy":
                self.tasks.appendleft(task)
                self.tasks.appendleft(self.charge(task.robot))
            else:
                # A fault, getting stuck, a blocked target: retry after the rescue.
                self.tasks.appendleft(task)
        elif kind == "command_rejected" and event["command_id"] == active_id:
            assert self.active is not None
            _, task = self.active
            self.active = None
            if not task.rescue:
                self.tasks.appendleft(task)
            self.backoff_until = event["t"] + BACKOFF_S

    def decide(self, observation: Message) -> Message:
        for r in observation["robots"]:
            self.robots[r["robot_id"]] = r
        for e in observation["events"]:
            self.handle(e)
        decision = self.decision(observation)
        if self.active is not None:
            return decision
        if observation["t"] < self.backoff_until:
            decision["wake_at"] = self.backoff_until
            return decision
        # A revived robot can be tried with the charger again later.
        self.charger_tried = {r for r in self.charger_tried
                              if self.robots.get(r, {}).get("status") == "offline"}
        rescue = self.rescue_task()
        if rescue is not None:
            self.issue(rescue, decision)
            return decision
        while not self.tasks and self.phase != "done" and self.pad is not None:
            self.advance_phase()
        if self.tasks:
            task = self.tasks.popleft()
            if task.action["type"] != "charge" and self.battery_low(task.robot):
                self.tasks.appendleft(task)
                task = self.charge(task.robot)
            self.issue(task, decision)
        return decision


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--serve", action="store_true",
                        help="speak NDJSON on stdin and stdout, for `arcadiabench run -- ...`")
    parser.add_argument("--seed", type=int, default=1)
    parser.add_argument("--preset", default="small", choices=arcadiabench.PRESETS)
    parser.add_argument("--variant", default="nominal", choices=arcadiabench.VARIANTS)
    args = parser.parse_args()
    if args.serve:
        return arcadiabench.serve(Builder())
    result = arcadiabench.run(Builder(), seed=args.seed, preset=args.preset, variant=args.variant)
    m = result["metrics"]
    print(json.dumps({
        "run_id": result["run_id"], "reason": result["reason"], "sols": round(m["sols"], 3),
        "speed": result["speed"], "faults": m["fleet"]["faults"],
        "policy_violations": m["autonomy"]["policy_violations"],
    }))
    return 0


if __name__ == "__main__":
    sys.exit(main())
