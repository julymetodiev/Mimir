#!/usr/bin/env python3
"""Architecture rule 10: every direct dependency is on its latest stable version.

Compares what the manifests declare with crates.io. `cargo outdated` compares the lock file, and
can miss a dependency a major version behind.
"""
import json, pathlib, sys, tomllib, urllib.request


def declared():
    root = tomllib.load(open("Cargo.toml", "rb"))
    found = {}
    tables = [root["workspace"]["dependencies"]]
    for manifest in pathlib.Path("crates").glob("*/Cargo.toml"):
        crate = tomllib.load(open(manifest, "rb"))
        for target in crate.get("target", {}).values():
            tables.append(target.get("dependencies", {}))
        tables += [crate.get(section, {}) for section in ("dependencies", "dev-dependencies", "build-dependencies")]
    for table in tables:
        for name, spec in table.items():
            if isinstance(spec, dict) and (spec.get("workspace") or "path" in spec):
                continue
            found[name] = spec if isinstance(spec, str) else spec["version"]
    return found


def latest(name):
    request = urllib.request.Request(f"https://crates.io/api/v1/crates/{name}", headers={"User-Agent": "mimir-check-deps"})
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.load(response)["crate"]["max_stable_version"]


def behind(version, newest):
    """A declared `1.2` is current only if the newest is the same in every part it names."""
    parts = version.lstrip("^=").split(".")
    return parts != newest.split("+")[0].split(".")[: len(parts)]


stale = [(name, version, latest(name)) for name, version in sorted(declared().items())]
stale = [entry for entry in stale if behind(entry[1], entry[2])]
for name, version, newest in stale:
    print(f"error: {name} is at {version}, the latest is {newest}")
if not stale:
    print("every dependency is on its latest version")
sys.exit(1 if stale else 0)
