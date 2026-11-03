#!/usr/bin/env python3
"""After edits, is a graph from a running analyzer the same as one from a new index?

Usage: edit_compare.py MIMIR PROJECT OUT_DIR FILE [FILE...]
PROJECT must be a git repository you can throw away: each FILE gets two comment lines on top
and the project is synced four times, ending with `sync --full`. Graphs land in OUT_DIR, and
graph_diff.py compares them. Prints time and peak memory, children included, per sync.
"""
import pathlib, re, shutil, subprocess, sys, time

mimir, project, out, *files = sys.argv[1:]
project, out = pathlib.Path(project), pathlib.Path(out)
out.mkdir(parents=True, exist_ok=True)


def sync(label, *args):
    started = time.time()
    run = subprocess.run(["/usr/bin/time", "-l", mimir, "sync", *args], cwd=project, capture_output=True, text=True)
    peak = re.search(r"(\d+)\s+maximum resident", run.stderr)
    print(f"{label}: {time.time() - started:.1f}s peak={int(peak.group(1)) / 2**30:.2f}GB exit={run.returncode}", flush=True)
    for line in (run.stdout + run.stderr).splitlines():
        if re.search(r"Calls:|Edited since|Functions used|Trait method", line):
            print("   ", line[:120], flush=True)
    shutil.copy(project / ".mimir/graph/graph.json", out / f"{label}.json")


subprocess.run([mimir, "init"], cwd=project, capture_output=True)
sync("base")
sync("cached")
for name in files:
    path = project / name
    path.write_text("// edited\n// edited\n" + path.read_text())
sync("edited")
sync("full", "--full")
print(f"Compare with: graph_diff.py {out}/edited.json {out}/full.json")
