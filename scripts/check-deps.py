#!/usr/bin/env python3
"""Architecture rule 10: every direct dependency is on its latest stable version.

Compares what the manifests declare with crates.io. `cargo outdated` compares the lock file, and
can miss a dependency a major version behind.
"""
import json, pathlib, re, sys, tomllib, urllib.request


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
    request = urllib.request.Request(f"https://crates.io/api/v1/crates/{name}", headers={"User-Agent": "latt-check-deps"})
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.load(response)["crate"]["max_stable_version"]


def behind(version, newest):
    """A declared `1.2` is current only if the newest is the same in every part it names."""
    parts = version.lstrip("^=").split(".")
    return parts != newest.split("+")[0].split(".")[: len(parts)]


stale = [(name, version, latest(name)) for name, version in sorted(declared().items())]
stale = [entry for entry in stale if behind(entry[1], entry[2])]

# `tsrs` and `pyrefly` run as programs, and the version each was measured with is in the code.
tested = re.search(r'TESTED_VERSION: &str = "(.+?)"', pathlib.Path("crates/latt-resolve-lsp/src/tsrs.rs").read_text()).group(1)
with urllib.request.urlopen("https://registry.npmjs.org/tsrs/latest", timeout=20) as response:
    newest_tsrs = json.load(response)["version"].split("-")[0]
if tested != newest_tsrs:
    stale.append(("tsrs", tested, newest_tsrs))
tested = re.search(r'TESTED_VERSION: &str = "(.+?)"', pathlib.Path("crates/latt-resolve-lsp/src/pyrefly.rs").read_text()).group(1)
with urllib.request.urlopen("https://pypi.org/pypi/pyrefly/json", timeout=20) as response:
    newest_pyrefly = json.load(response)["info"]["version"]
if tested != newest_pyrefly:
    stale.append(("pyrefly", tested, newest_pyrefly))
# So do `scip-go` and `gopls`, whose newest version is what the Go proxy says.
tools = pathlib.Path("crates/latt-resolve-go/src/tools.rs").read_text()
for name, module in (("scip-go", "github.com/scip-code/scip-go"), ("gopls", "golang.org/x/tools/gopls")):
    tested = re.search(rf'{name.upper().replace("-", "_")}_TESTED: &str = "(.+?)"', tools).group(1)
    with urllib.request.urlopen(f"https://proxy.golang.org/{module}/@latest", timeout=20) as response:
        newest = json.load(response)["Version"].lstrip("v")
    if tested != newest:
        stale.append((name, tested, newest))
# So does `clangd-indexer`, whose newest version is what clangd's releases say.
tools = pathlib.Path("crates/latt-resolve-clang/src/tools.rs").read_text()
tested = re.search(r'CLANGD_INDEXER_TESTED: &str = "(.+?)"', tools).group(1)
with urllib.request.urlopen("https://api.github.com/repos/clangd/clangd/releases/latest", timeout=20) as response:
    newest = json.load(response)["tag_name"]
if tested != newest:
    stale.append(("clangd-indexer", tested, newest))
for name, version, newest in stale:
    print(f"error: {name} is at {version}, the latest is {newest}")
if not stale:
    print("every dependency is on its latest version")
sys.exit(1 if stale else 0)
