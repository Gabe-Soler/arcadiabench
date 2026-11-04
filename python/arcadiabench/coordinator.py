"""Writing a coordinator in Python.

Subclass :class:`Coordinator` and override :meth:`Coordinator.decide`. Run it
in process with :func:`arcadiabench.run`, or as a subprocess of the
``arcadiabench`` command line through :func:`serve`::

    import arcadiabench

    class Mine(arcadiabench.Coordinator):
        name = "mine"

        def decide(self, observation):
            return self.decision(observation)

    if __name__ == "__main__":
        arcadiabench.serve(Mine())

Messages are plain dicts with the shapes of arcadia-protocol's JSON Schemas.
"""

from __future__ import annotations

import json
import sys
from typing import IO, Any, Dict, Iterable, Optional

from ._native import PROTOCOL_VERSION

Message = Dict[str, Any]

__all__ = ["Coordinator", "serve"]


class Coordinator:
    """Base class for Python coordinators.

    A coordinator gets one ``hello`` per run, then one ``observation`` per
    decision point, and finally ``end``. The defaults welcome every run and
    never act; override :meth:`decide` (and :meth:`welcome` to read the
    scenario). Any object with the same three methods works too.
    """

    #: The name the coordinator reports in its welcome.
    name: str = "python-coordinator"
    #: The version the coordinator reports in its welcome.
    version: str = "0.0.0"

    def welcome(self, hello: Message) -> Message:
        """Start a run. Returns the ``welcome`` message."""
        return {
            "type": "welcome",
            "protocol_version": PROTOCOL_VERSION,
            "coordinator": {"name": self.name, "version": self.version},
        }

    def decide(self, observation: Message) -> Message:
        """Answer an observation with a ``decision``. The default does nothing."""
        return self.decision(observation)

    def end(self, end: Message) -> None:
        """The run is over: ``end`` carries its time and reason."""

    @staticmethod
    def decision(
        observation: Message,
        commands: Iterable[Message] = (),
        to_earth: Iterable[Message] = (),
        wake_at: Optional[float] = None,
    ) -> Message:
        """A ``decision`` answering ``observation``."""
        d: Message = {
            "type": "decision",
            "seq": observation["seq"],
            "commands": list(commands),
            "to_earth": list(to_earth),
        }
        if wake_at is not None:
            d["wake_at"] = wake_at
        return d


def _complete(reply: Any, kind: str, method: str) -> Message:
    """Fill in what a reply may leave out, as the in-process link does."""
    if not isinstance(reply, dict):
        raise TypeError(f"{method}() must return a dict, not {type(reply).__name__}")
    message: Message = dict(reply)
    message.setdefault("type", kind)
    if kind == "welcome" and message["type"] == "welcome":
        message.setdefault("protocol_version", PROTOCOL_VERSION)
    return message


def serve(
    coordinator: Any,
    stdin: Optional[IO[str]] = None,
    stdout: Optional[IO[str]] = None,
) -> int:
    """Serve one run over newline-delimited JSON on stdin and stdout.

    This is how a Python coordinator runs under the command line, for example
    ``arcadiabench run -- python my_coordinator.py``. While it serves, anything
    the coordinator prints goes to stderr, so it cannot corrupt the protocol.

    Returns 0 once the run ends, and 1 if the simulator sends an error or the
    input closes first.
    """
    reader = stdin if stdin is not None else sys.stdin
    writer = stdout if stdout is not None else sys.stdout
    saved = sys.stdout
    if stdout is None:
        sys.stdout = sys.stderr
    try:
        for line in reader:
            if not line.strip():
                continue
            message = json.loads(line)
            kind = message.get("type")
            if kind == "hello":
                reply = _complete(coordinator.welcome(message), "welcome", "welcome")
            elif kind == "observation":
                reply = _complete(coordinator.decide(message), "decision", "decide")
            elif kind == "end":
                if hasattr(coordinator, "end"):
                    coordinator.end(message)
                return 0
            elif kind == "error":
                print(f"arcadiabench: {message.get('message', '')}", file=sys.stderr)
                return 1
            else:
                raise ValueError(f"unexpected message type {kind!r}")
            writer.write(json.dumps(reply, separators=(",", ":"), allow_nan=False) + "\n")
            writer.flush()
        return 1
    finally:
        sys.stdout = saved

