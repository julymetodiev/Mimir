#!/usr/bin/env python3
"""How long does trace take through the daemon, from no daemon on, and after edits?

Usage: daemon_latency.py LATT PROJECT SYMBOL FILE
PROJECT must be a git repository you can throw away. The daemon starts by itself with the first
command, so the script stops any running one first; the first trace then pays for the start.
FILE gets a comment line on top, three times.
"""
import pathlib, re, subprocess, sys, tempfile, time

latt, project, symbol, file = sys.argv[1:]
project = pathlib.Path(project)


def run(*args):
    started = time.time()
    # To a file, not a pipe: a pipe is read to its end, which waits for every process holding it.
    with tempfile.TemporaryFile("w+") as out:
        subprocess.run([latt, *args], cwd=project, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, timeout=300)
        out.seek(0)
        return time.time() - started, out.read()


def trace():
    took, out = run("trace", "-s", symbol, "--callers", "--depth", "1")
    callers = re.search(r"Callers \((\d+)\)", out)
    return took, int(callers.group(1)) if callers else 0, "Partial:" in out


run("daemon", "stop")
run("init")
started = time.time()

took, callers, partial = trace()
print(f"first answer from no daemon: {took:.2f}s callers={callers} partial={partial}", flush=True)
while partial and time.time() - started < 600:
    time.sleep(3)
    took, callers, partial = trace()
print(f"complete {time.time() - started:.0f}s after start: callers={callers}", flush=True)

path = project / file
for edit in range(1, 4):
    path.write_text("// edited\n" + path.read_text())
    time.sleep(1.5)
    took, callers, partial = trace()
    print(f"edit {edit}: trace {took:.2f}s callers={callers} partial={partial}", flush=True)
    settle = time.time()
    while partial and time.time() - settle < 120:
        time.sleep(1)
        took, callers, partial = trace()
    if time.time() - settle > 0.5:
        print(f"   complete after {time.time() - settle:.1f}s callers={callers}", flush=True)
print(run("daemon", "stop")[1].strip()[:60])
