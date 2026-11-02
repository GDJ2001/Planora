#!/usr/bin/env python3
"""Record bounded Planora process growth from /proc without names or arguments."""

import argparse
import csv
import json
import os
import time
from collections import defaultdict
from pathlib import Path


def snapshot(elapsed):
    rows = []
    ticks_per_second = os.sysconf("SC_CLK_TCK")
    for entry in os.scandir("/proc"):
        if not entry.name.isdigit():
            continue
        base = Path(entry.path)
        try:
            name = (base / "comm").read_text().strip()
            if not name.startswith("planora"):
                continue
            stat = (base / "stat").read_text().rsplit(") ", 1)[1].split()
            status = (base / "status").read_text().splitlines()
            fields = {}
            for line in status:
                key, _, value = line.partition(":")
                if key in {"VmRSS", "VmSize", "Threads"}:
                    fields[key] = int(value.strip().split()[0])
            rows.append(
                {
                    "elapsed_seconds": round(elapsed, 3),
                    "name": name,
                    "pid": int(entry.name),
                    "start_ticks": int(stat[19]),
                    "cpu_seconds": round((int(stat[11]) + int(stat[12])) / ticks_per_second, 3),
                    "rss_bytes": fields.get("VmRSS", 0) * 1024,
                    "virtual_bytes": fields.get("VmSize", 0) * 1024,
                    "threads": fields.get("Threads", 0),
                    "file_descriptors": len(list((base / "fd").iterdir())),
                }
            )
        except (FileNotFoundError, PermissionError, ValueError, IndexError):
            # A process can exit between directory enumeration and the reads.
            continue
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--duration-seconds", type=int, default=3600)
    parser.add_argument("--interval-seconds", type=int, default=10)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if not 30 <= args.duration_seconds <= 14400:
        parser.error("duration must be 30..14400 seconds")
    if not 1 <= args.interval_seconds <= 60:
        parser.error("interval must be 1..60 seconds")
    args.output.parent.mkdir(parents=True, exist_ok=True)

    samples = []
    started = time.monotonic()
    while (elapsed := time.monotonic() - started) < args.duration_seconds:
        samples.extend(snapshot(elapsed))
        time.sleep(args.interval_seconds)

    columns = [
        "elapsed_seconds", "name", "pid", "start_ticks", "cpu_seconds",
        "rss_bytes", "virtual_bytes", "threads", "file_descriptors",
    ]
    with args.output.open("w", newline="", encoding="utf-8") as output:
        writer = csv.DictWriter(output, fieldnames=columns)
        writer.writeheader()
        writer.writerows(samples)

    groups = defaultdict(list)
    for row in samples:
        groups[(row["name"], row["pid"], row["start_ticks"])].append(row)
    summary = []
    for (name, pid, start_ticks), rows in groups.items():
        first, last = rows[0], rows[-1]
        summary.append(
            {
                "name": name,
                "pid": pid,
                "start_ticks": start_ticks,
                "samples": len(rows),
                "duration_seconds": round(last["elapsed_seconds"] - first["elapsed_seconds"], 1),
                "cpu_seconds_delta": round(last["cpu_seconds"] - first["cpu_seconds"], 3),
                "rss_bytes_delta": last["rss_bytes"] - first["rss_bytes"],
                "virtual_bytes_delta": last["virtual_bytes"] - first["virtual_bytes"],
                "threads_delta": last["threads"] - first["threads"],
                "file_descriptors_delta": last["file_descriptors"] - first["file_descriptors"],
                "peak_rss_bytes": max(row["rss_bytes"] for row in rows),
                "peak_file_descriptors": max(row["file_descriptors"] for row in rows),
            }
        )
    summary_path = args.output.with_suffix(".json")
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(f"Samples: {args.output}\nSummary: {summary_path}")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
