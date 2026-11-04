"""``python -m arcadiabench``: the ``arcadiabench`` command line."""

from __future__ import annotations

import signal
import sys
from typing import List, Optional

from . import _native


def main(argv: Optional[List[str]] = None) -> int:
    """Run the command line with ``argv`` (default: ``sys.argv[1:]``)."""
    # Ctrl-C stops the command at once, as it does the native binary.
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    return _native.cli(sys.argv[1:] if argv is None else list(argv))


if __name__ == "__main__":
    sys.exit(main())
