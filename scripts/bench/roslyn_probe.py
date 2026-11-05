#!/usr/bin/env python3
"""Check Roslyn definitions on a disposable C# fixture, without a latt graph.

Usage: python3 scripts/bench/roslyn_probe.py --server /path/to/roslyn-language-server
The fixture has no package dependencies. Only the temporary copy is restored or edited.
"""

import argparse
import json
import os
from pathlib import Path
import queue
import re
import shutil
import sys
import subprocess
import tempfile
import threading
import time
from urllib.parse import unquote, urlparse


MARKER = re.compile(r"/\*(def|site):([\w-]+)\*/")
READY = "workspace/projectInitializationComplete"


def positions(source, kind):
    """LSP columns are UTF-16 code units, not Python characters or UTF-8 bytes."""
    result = {}
    for marker in MARKER.finditer(source):
        if marker[1] != kind:
            continue
        before = source[:marker.end()]
        line = before.count("\n")
        column = len(before.rsplit("\n", 1)[-1].encode("utf-16-le")) // 2
        if marker[2] in result:
            raise ValueError(f"Duplicate {kind} marker: {marker[2]}")
        result[marker[2]] = {"line": line, "character": column}
    return result


class Client:
    def __init__(self, server, root, log, timeout):
        self.timeout = timeout
        self.messages = queue.Queue()
        self.notifications = []
        self.next_id = 0
        self.process = subprocess.Popen(
            [server, "--stdio", "--autoLoadProjects", "--telemetryLevel", "off"],
            cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log,
        )
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            while True:
                length = None
                while True:
                    line = self.process.stdout.readline()
                    if not line:
                        raise EOFError("Roslyn closed stdout")
                    if line in (b"\r\n", b"\n"):
                        break
                    key, value = line.split(b":", 1)
                    if key.lower() == b"content-length":
                        length = int(value)
                if length is None or length < 0:
                    raise ValueError("Invalid Content-Length")
                body = self.process.stdout.read(length)
                if len(body) != length:
                    raise EOFError("Incomplete LSP message")
                self.messages.put(json.loads(body))
        except (OSError, ValueError, EOFError) as error:
            self.messages.put(error)

    def send(self, message):
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def notify(self, method, params):
        self.send({"method": method, "params": params})

    def receive(self, deadline):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError("Roslyn response timed out")
        try:
            message = self.messages.get(timeout=remaining)
        except queue.Empty as error:
            raise TimeoutError("Roslyn response timed out") from error
        if isinstance(message, Exception):
            raise message
        if "method" in message:
            if "id" not in message:
                self.notifications.append(message)
            else:
                method = message["method"]
                if method == "workspace/configuration":
                    result = [None for _ in message["params"]["items"]]
                elif method in ("client/registerCapability", "window/workDoneProgress/create"):
                    result = None
                else:
                    self.send({"id": message["id"], "error": {
                        "code": -32601, "message": f"Unsupported client method: {method}"}})
                    return None
                self.send({"id": message["id"], "result": result})
            return None
        return message

    def request(self, method, params):
        self.next_id += 1
        request_id = self.next_id
        self.send({"id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + self.timeout
        while True:
            message = self.receive(deadline)
            if message is None:
                continue
            if message.get("id") != request_id:
                raise ValueError(f"Unexpected response: {message}")
            if "error" in message:
                raise RuntimeError(f"{method}: {message['error']}")
            return message["result"]

    def initialize(self, root):
        result = self.request("initialize", {
            "processId": os.getpid(), "rootUri": root.as_uri(),
            "workspaceFolders": [{"uri": root.as_uri(), "name": "probe"}],
            "capabilities": {
                "general": {"positionEncodings": ["utf-16"]},
                "workspace": {"configuration": True},
                "window": {"workDoneProgress": True},
                "textDocument": {"definition": {"linkSupport": False}},
            },
        })
        self.notify("initialized", {})
        deadline = time.monotonic() + self.timeout
        while not any(n["method"] == READY for n in self.notifications):
            self.receive(deadline)
        return result

    def open(self, path, source):
        self.notify("textDocument/didOpen", {"textDocument": {
            "uri": path.as_uri(), "languageId": "csharp", "version": 1, "text": source}})

    def change(self, path, source, previous, version):
        start = 0
        while start < min(len(source), len(previous)) and source[start] == previous[start]:
            start += 1
        suffix = 0
        while (suffix < min(len(source), len(previous)) - start
               and source[-suffix - 1] == previous[-suffix - 1]):
            suffix += 1
        def position(text, offset):
            prefix = text[:offset]
            return {"line": prefix.count("\n"),
                    "character": len(prefix.rsplit("\n", 1)[-1].encode("utf-16-le")) // 2}
        self.notify("textDocument/didChange", {
            "textDocument": {"uri": path.as_uri(), "version": version},
            "contentChanges": [{"range": {"start": position(previous, start),
                "end": position(previous, len(previous) - suffix)},
                "text": source[start:len(source) - suffix]}]})

    def stop(self):
        try:
            if self.process.poll() is None:
                self.request("shutdown", None)
                self.notify("exit", None)
                self.process.wait(timeout=10)
        finally:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=10)
            self.process.stdin.close()
            self.process.stdout.close()
            self.reader.join(timeout=2)


def locations(answer, root):
    result = []
    for location in (answer if isinstance(answer, list) else [answer]) if answer else []:
        uri = location.get("uri", location.get("targetUri"))
        parsed = urlparse(uri)
        if parsed.scheme != "file":
            raise ValueError(f"Unexpected URI scheme: {parsed.scheme}")
        path = Path(unquote(parsed.path)).resolve()
        try:
            relative = str(path.relative_to(root))
        except ValueError:
            relative = "<external>"
        span = location.get("targetSelectionRange", location.get("range"))
        result.append([relative, span["start"]["line"], span["start"]["character"]])
    return sorted(result)


def check(client, root, sources, cross_target):
    definitions = {}
    for path, source in sources.items():
        for name, position in positions(source, "def").items():
            if name in definitions:
                raise ValueError(f"Duplicate definition: {name}")
            definitions[name] = [path, position["line"], position["character"]]
    aliases = {"unicode": "int", "cross-file": cross_target}
    results = {}
    for path, source in sources.items():
        for name, position in positions(source, "site").items():
            answer = client.request("textDocument/definition", {
                "textDocument": {"uri": (root / path).as_uri()}, "position": position})
            actual = locations(answer, root)
            expected = [] if name == "dynamic" else [definitions[aliases.get(name, name)]]
            results[name] = {"actual": actual, "expected": expected, "pass": actual == expected}
    if not results:
        raise ValueError("The fixture has no sites")
    return results


def run(args, root, log):
    fixture = Path(__file__).resolve().parents[2] / "fixtures/csharp-workspace"
    shutil.copytree(fixture, root, dirs_exist_ok=True, ignore=shutil.ignore_patterns("bin", "obj"))
    subprocess.run(["dotnet", "build", "Probe.csproj", "--nologo", "-v:q", "-m:2"],
                   cwd=root, stdout=log, stderr=log, check=True, timeout=args.timeout)
    sources = {str(path.relative_to(root)): path.read_text() for path in sorted(root.rglob("*.cs"))
               if not {"obj", "bin"}.intersection(path.relative_to(root).parts)}
    report = {"phases": {}}
    client = Client(args.server, root, log, args.timeout)
    try:
        start = time.monotonic()
        report["initialize"] = client.initialize(root)
        report["ready_seconds"] = round(time.monotonic() - start, 3)
        for path, source in sources.items():
            client.open(root / path, source)
        report["phases"]["initial"] = check(client, root, sources, "alpha")

        previous = sources["Targets.cs"]
        sources["Targets.cs"] = previous.replace(
            "public static Alpha ", "public static Beta ")
        client.change(root / "Targets.cs", sources["Targets.cs"], previous, 2)
        report["phases"]["cross_file_edit"] = check(client, root, sources, "beta")

        previous = sources["Targets.cs"]
        sources["Targets.cs"] = "// moved declarations\n" + previous
        client.change(root / "Targets.cs", sources["Targets.cs"], previous, 3)
        report["phases"]["declaration_move"] = check(client, root, sources, "beta")
    finally:
        client.stop()
    for path, source in sources.items():
        (root / path).write_text(source)
    client = Client(args.server, root, log, args.timeout)
    try:
        client.initialize(root)
        for path, source in sources.items():
            client.open(root / path, source)
        report["phases"]["restart"] = check(client, root, sources, "beta")
    finally:
        client.stop()
    report["restart_equal"] = report["phases"]["declaration_move"] == report["phases"]["restart"]
    report["passed"] = sum(row["pass"] for phase in report["phases"].values() for row in phase.values())
    report["total"] = sum(len(phase) for phase in report["phases"].values())
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", default="roslyn-language-server")
    parser.add_argument("--timeout", type=float, default=120)
    args = parser.parse_args()
    server = shutil.which(args.server)
    if server is None:
        parser.error("roslyn-language-server is not installed")
    args.server = str(Path(server).resolve())
    with tempfile.TemporaryDirectory(prefix="latt-roslyn-") as directory, tempfile.TemporaryFile(mode="w+") as log:
        try:
            report = run(args, Path(directory).resolve(), log)
        except Exception:
            log.seek(0)
            print(log.read()[-12000:], file=sys.stderr)
            raise
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] == report["total"] and report["restart_equal"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
