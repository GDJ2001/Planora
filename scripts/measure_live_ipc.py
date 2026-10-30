#!/usr/bin/env python3
"""Read-only, redacted live helper latency probe for Windows or Linux.

The probe requests a sampling interval only for its own connection. Closing
the connection removes that request. It exports no process or machine names.
"""

import argparse
import json
import os
import socket
import struct
import time
from collections import defaultdict
from pathlib import Path


class Connection:
    def __init__(self):
        if os.name == "nt":
            self.stream = open(r"\\.\pipe\planora-v2", "r+b", buffering=0)
        else:
            self.stream = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            self.stream.settimeout(5)
            self.stream.connect("/run/planora/planora-v2.sock")

    def close(self):
        self.stream.close()

    def read_exact(self, length):
        parts = []
        remaining = length
        while remaining:
            if os.name == "nt":
                part = self.stream.read(remaining)
            else:
                part = self.stream.recv(remaining)
            if not part:
                raise EOFError("helper closed the connection")
            parts.append(part)
            remaining -= len(part)
        return b"".join(parts)

    def request(self, request_id, command):
        payload = json.dumps({"request_id": request_id, "command": command}).encode()
        frame = struct.pack("<I", len(payload)) + payload
        if os.name == "nt":
            self.stream.write(frame)
        else:
            self.stream.sendall(frame)
        response_len = struct.unpack("<I", self.read_exact(4))[0]
        if response_len > 8 * 1024 * 1024:
            raise ValueError("helper response exceeds the probe limit")
        return json.loads(self.read_exact(response_len))["response"]


def stats(values):
    ordered = sorted(values)
    if not ordered:
        return {"count": 0}
    return {
        "count": len(ordered),
        "p50_ms": round(ordered[int(0.50 * (len(ordered) - 1))], 3),
        "p95_ms": round(ordered[int(0.95 * (len(ordered) - 1))], 3),
        "max_ms": round(ordered[-1], 3),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", type=int, default=100)
    parser.add_argument("--interval-ms", type=int, default=100)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 2 <= args.samples <= 600:
        parser.error("samples must be between 2 and 600")
    if args.interval_ms not in {100, 250, 500, 1000, 2000}:
        parser.error("interval must be a supported helper interval")

    connection = Connection()
    try:
        hello = connection.request(1, {"Hello": {"version": 6}})["Hello"]
        status = connection.request(
            2, {"SetSamplingInterval": {"interval_ms": args.interval_ms}}
        )["SamplingStatus"]
        values = defaultdict(list)
        sequences = []
        process_counts = []
        for index in range(args.samples):
            frame = connection.request(3 + index, "Snapshot")["Snapshot"]
            sequences.append(frame["sequence"])
            process_counts.append(len(frame["processes"]))
            values["collection_total"].append(frame["collection_duration_ms"])
            for provider, duration in frame["provider_durations_ms"].items():
                values[provider].append(duration)
            time.sleep(args.interval_ms / 1000 + 0.01)
    finally:
        connection.close()

    report = {
        "protocol_version": hello["version"],
        "authorized": hello["helper_authorized"],
        "platform": "windows" if os.name == "nt" else "linux",
        "requested_interval_ms": args.interval_ms,
        "effective_interval_ms": status["effective_ms"],
        "samples": args.samples,
        "distinct_sequences": len(set(sequences)),
        "process_count_min": min(process_counts),
        "process_count_max": max(process_counts),
        "latencies": {name: stats(items) for name, items in values.items()},
    }
    output = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(output, encoding="utf-8")
    print(output, end="")


if __name__ == "__main__":
    main()
