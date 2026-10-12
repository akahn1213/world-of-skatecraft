#!/usr/bin/env python3
"""Restore the pinned Unit.cpp's CRLF endings in an otherwise portable patch."""

from pathlib import Path
import sys


def prepare(data):
    unit = False
    hunk = False
    lines = []
    for line in data.splitlines(keepends=True):
        if line.startswith(b"diff --git "):
            unit = line.startswith(b"diff --git a/src/game/Objects/Unit.cpp ")
            hunk = False
        elif line.startswith(b"@@"):
            hunk = True
        if unit and hunk and line[:1] in (b" ", b"+", b"-"):
            line = line.rstrip(b"\r\n") + b"\r\n"
        lines.append(line)
    return b"".join(lines)


if __name__ == "__main__":
    Path(sys.argv[2]).write_bytes(prepare(Path(sys.argv[1]).read_bytes()))
