#!/usr/bin/env python3
"""How long does trace take through the daemon, from no index on, and after edits?

Usage: daemon_latency.py MIMIR PROJECT SYMBOL FILE
PROJECT must be a git repository you can throw away, with a short path: the daemon's socket
is inside it and the limit is about 100 characters. FILE gets a comment line on top, three times.
"""
import pathlib, re, subprocess, sys, tempfile, time

mimir, project, symbol, file = sys.argv[1:]
project = pathlib.Path(project)


def run(*args):
    started = time.time()
    # To a file: the daemon inherits a pipe, and reading one would wait for the daemon to end.
    with tempfile.TemporaryFile("w+") as out:
        subprocess.run([mimir, *args], cwd=project, stdout=out, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, timeout=300)
        out.seek(0)
        return time.time() - started, out.read()


def trace():
    took, out = run("trace", "-s", symbol, "--callers", "--depth", "1")
    callers = re.search(r"Callers \((\d+)\)", out)
    return took, int(callers.group(1)) if callers else 0, "Partial:" in out


run("init")
with tempfile.TemporaryFile("w") as log:
    started = time.time()
    subprocess.Popen([mimir, "daemon", "start"], cwd=project, stdout=log, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL, start_new_session=True)
while "Daemon: running" not in run("daemon", "status")[1] and time.time() - started < 120:
    time.sleep(0.5)
print(f"daemon running after {time.time() - started:.1f}s", flush=True)

took, callers, partial = trace()
print(f"first trace: {took:.2f}s callers={callers} partial={partial}", flush=True)
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
