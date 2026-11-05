#!/usr/bin/env python3
"""After edits, is a graph from a running analyzer the same as one from a new index?

Usage: edit_compare.py LATT PROJECT OUT_DIR FILE [FILE...]
PROJECT must be a git repository you can throw away: each FILE gets two comment lines on top
and the project is synced four times, ending with `sync --full`. Graphs land in OUT_DIR, and
graph_diff.py compares them. Prints time and peak memory per sync. The memory is the sum over
latt and rust-analyzer processes (the daemon and its analyzer included), sampled every 0.2 s.
The daemon is stopped before and after, so the first sync starts from none.
"""
import pathlib, re, shutil, subprocess, sys, tempfile, time

latt, project, out, *files = sys.argv[1:]
project, out = pathlib.Path(project), pathlib.Path(out)
out.mkdir(parents=True, exist_ok=True)


def rss_kb():
    """Summed resident set size, in KB, of processes named latt or containing rust-analyzer."""
    total = 0
    for line in subprocess.run(["ps", "-axo", "rss=,comm="], capture_output=True, text=True).stdout.splitlines():
        fields = line.split(None, 1)
        if len(fields) == 2 and (fields[1].endswith("/latt") or "rust-analyzer" in fields[1]):
            total += int(fields[0])
    return total


def sync(label, *args):
    started = time.time()
    peak = 0
    # To a file, not a pipe: a pipe is read to its end, which waits for every process holding it.
    with tempfile.TemporaryFile("w+") as log:
        proc = subprocess.Popen([latt, "sync", "--json", *args], cwd=project, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
        while True:
            peak = max(peak, rss_kb())
            try:
                proc.wait(timeout=0.2)
                break
            except subprocess.TimeoutExpired:
                pass
        peak = max(peak, rss_kb())  # after exit: a daemon this sync left running still counts
        log.seek(0)
        text = log.read()
    print(f"{label}: {time.time() - started:.1f}s peak={peak / 2**20:.2f}GB exit={proc.returncode}", flush=True)
    for line in text.splitlines():
        if re.search(r"Calls:|Edited since|Functions used|Trait method", line):
            print("   ", line[:120], flush=True)
    shutil.copy(project / ".latt/graph/graph.json", out / f"{label}.json")


subprocess.run([latt, "daemon", "stop"], cwd=project, capture_output=True)
subprocess.run([latt, "init"], cwd=project, capture_output=True)
sync("base")
sync("cached")
for name in files:
    path = project / name
    comment = "#" if path.suffix == ".py" else "//"
    path.write_text(f"{comment} edited\n{comment} edited\n" + path.read_text())
sync("edited")
sync("full", "--full")
subprocess.run([latt, "daemon", "stop"], cwd=project, capture_output=True)
print(f"Compare with: graph_diff.py {out}/edited.json {out}/full.json")
