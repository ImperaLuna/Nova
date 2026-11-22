#!/usr/bin/env python3
"""Late frames from surface commits (IMP-369 prototype).

Reads a WAYLAND_DEBUG=client log on stdin (or a file), finds every
wl_surface commit, groups each surface's commits into motions (a pause of
more than --pause ms ends one), and counts a commit late when it comes more
than 1.5 refresh intervals after the one before inside a motion.

  late-frames.py --refresh-hz 144 run.log
"""

import argparse
import re
import sys
from collections import defaultdict

COMMIT = re.compile(r"\[\s*([\d.]+)\]\s*(?:\{[^}]*\}\s*)?->\s*wl_surface[#@](\d+)\.commit\(")
ATTACH = re.compile(r"->\s*wl_surface[#@](\d+)\.attach\(")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("log", nargs="?")
    parser.add_argument("--refresh-hz", type=float, required=True)
    parser.add_argument("--pause", type=float, default=100.0, help="ms that end a motion")
    parser.add_argument("--min-commits", type=int, default=20, help="ignore surfaces with fewer")
    args = parser.parse_args()

    source = open(args.log, errors="replace") if args.log else sys.stdin
    # Only commits that carry a new buffer are frames; GPUI also commits
    # alone after changing the input region.
    times = defaultdict(list)
    attached = set()
    for line in source:
        a = ATTACH.search(line)
        if a:
            attached.add(a.group(1))
            continue
        m = COMMIT.search(line)
        if m and m.group(2) in attached:
            attached.discard(m.group(2))
            times[m.group(2)].append(float(m.group(1)))

    interval = 1000.0 / args.refresh_hz
    total_frames = total_late = 0
    for surface, stamps in sorted(times.items(), key=lambda kv: -len(kv[1])):
        if len(stamps) < args.min_commits:
            continue
        frames = late = motions = 0
        worst = 0.0
        for prev, cur in zip(stamps, stamps[1:]):
            gap = cur - prev
            if gap > args.pause:
                motions += 1
                continue
            frames += 1
            worst = max(worst, gap)
            if gap > 1.5 * interval:
                late += 1
        total_frames += frames
        total_late += late
        print(
            f"surface {surface}: commits={len(stamps)} motions={motions + 1} "
            f"frames={frames} late={late} ({100 * late / max(frames, 1):.2f}%) worst={worst:.1f}ms"
        )
    print(
        f"all: frames={total_frames} late={total_late} "
        f"({100 * total_late / max(total_frames, 1):.2f}%) refresh={interval:.2f}ms"
    )


if __name__ == "__main__":
    main()
