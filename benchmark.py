#!/usr/bin/env python3
"""Reproducible metadata-only benchmark. All fixtures are temporary sparse files."""
import argparse
import json
import pathlib
import statistics
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser()
parser.add_argument("--files", type=int, default=100_000)
parser.add_argument("--binary", default=str(pathlib.Path(__file__).parent / "target/release/disko"))
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix="disko-bench-", dir="/tmp") as temp:
    base = pathlib.Path(temp)
    tree = base / "tree"
    tree.mkdir()
    state = base / "state"
    state.mkdir()
    db = state / "index.db"
    dirs = [tree / f"dir-{n:04d}" for n in range(200)]
    for directory in dirs:
        directory.mkdir()
    for n in range(args.files):
        with (dirs[n % len(dirs)] / f"file-{n:08d}").open("wb") as file:
            file.truncate((n * 7919) % (1 << 30))
    command = [args.binary, "--db", str(db), "--json"]
    start = time.perf_counter()
    subprocess.run(command + ["scan", str(tree)], check=True, capture_output=True)
    scan_seconds = time.perf_counter() - start
    results = {}
    queries = {
        "du": ["du", str(tree)],
        "largest_global": ["largest", "--apparent"],
        "largest_scoped": ["largest", str(tree), "--apparent"],
        "prefix_find": ["find", "file-0009"],
        "info": ["info", str(tree)],
    }
    for name, query in queries.items():
        timings = []
        for _ in range(30):
            start = time.perf_counter()
            subprocess.run(command + query, check=True, capture_output=True)
            timings.append((time.perf_counter() - start) * 1000)
        timings.sort()
        results[name] = {"median_ms": round(statistics.median(timings), 3), "p95_ms": round(timings[28], 3)}
    print(json.dumps({"files": args.files, "scan_seconds": round(scan_seconds, 3), "files_per_second": round(args.files / scan_seconds), "database_bytes": db.stat().st_size, "queries_including_process_startup": results}, indent=2))
