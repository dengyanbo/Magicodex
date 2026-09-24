"""Writes the licenses of the Rust crates compiled into a release binary.

Usage: python scripts/Write-ThirdPartyNotices.py <Cargo.toml> <output.md> [--target T] [--title T]
       [--extra part.md ...]

Only normal dependencies for the target (x86_64-pc-windows-msvc by default) are listed: build scripts, proc
macros and dev dependencies are not part of the shipped executable. Identical license
texts are printed once, followed by the crates that use them.
"""

import argparse
import hashlib
import json
import os
import subprocess
from pathlib import Path

REGISTRY = [
    "--config", 'source.crates-io.replace-with="official-github"',
    "--config",
    'source.official-github.registry="sparse+https://raw.githubusercontent.com/rust-lang/crates.io-index/master/"',
]
LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "unlicense", "copyright")


def metadata(manifest: Path, target: str) -> dict:
    env = dict(os.environ)
    env.setdefault("RUSTUP_TOOLCHAIN", "1.95.0-x86_64-pc-windows-msvc")
    cargo_home = Path(os.environ["CARGO_HOME"]) if os.environ.get("CARGO_HOME") else Path.home() / ".cargo"
    cargo = cargo_home / "bin" / "cargo.exe"
    result = subprocess.run(
        [str(cargo), "metadata", "--format-version", "1", "--filter-platform", target,
         "--offline", "--manifest-path", str(manifest), *REGISTRY],
        capture_output=True, env=env, check=True)
    return json.loads(result.stdout)


def shipped_packages(meta: dict) -> list[dict]:
    packages = {package["id"]: package for package in meta["packages"]}
    nodes = {node["id"]: node for node in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    # Proc macros run in the compiler and are not linked into the executable, and neither
    # is anything only they depend on.
    def proc_macro(pid: str) -> bool:
        return any("proc-macro" in target["kind"] for target in packages[pid]["targets"])

    seen, stack = set(), [root]
    while stack:
        current = stack.pop()
        if current in seen or proc_macro(current):
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    seen.discard(root)
    shipped = [packages[pid] for pid in seen if packages[pid].get("source")]
    return sorted(shipped, key=lambda p: (p["name"], p["version"]))


def license_files(package: dict) -> list[Path]:
    directory = Path(package["manifest_path"]).parent
    files = []
    if package.get("license_file"):
        files.append(directory / package["license_file"])
    for entry in sorted(directory.iterdir()):
        if entry.is_file() and entry.name.lower().startswith(LICENSE_PREFIXES) and entry not in files:
            files.append(entry)
    return files


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--target", default="x86_64-pc-windows-msvc")
    parser.add_argument("--title", default="Third-party notices")
    parser.add_argument("--extra", type=Path, action="append", default=[],
                        help="Markdown placed before the crate list")
    args = parser.parse_args()

    meta = metadata(args.manifest.resolve(), args.target)
    packages = shipped_packages(meta)
    texts: dict[str, dict] = {}
    rows = []
    for package in packages:
        label = f"{package['name']} {package['version']}"
        files = license_files(package)
        rows.append(f"| {package['name']} | {package['version']} | {package.get('license') or 'see text'} |")
        if not files:
            key = f"spdx:{package.get('license')}"
            texts.setdefault(key, {"title": None, "body": None, "users": []})["users"].append(label)
            continue
        for file in files:
            body = file.read_text(encoding="utf-8", errors="replace").replace("\r\n", "\n").strip()
            key = hashlib.sha256(body.encode()).hexdigest()
            entry = texts.setdefault(key, {"title": file.name, "body": body, "users": []})
            entry["users"].append(label)

    out = [f"# {args.title}", ""]
    for extra in args.extra:
        out += [extra.read_text(encoding="utf-8").strip(), ""]
    out += [
        "## Rust crates compiled into the executable", "",
        f"Target `{args.target}`, normal dependencies only.", "",
        "| Crate | Version | License |", "| --- | --- | --- |", *rows, "",
        "## License texts", "",
    ]
    for entry in texts.values():
        users = ", ".join(entry["users"])
        if entry["body"] is None:
            out += [f"### {users}", "",
                    "The crate ships no license file; its manifest declares the license shown above.", ""]
            continue
        out += [f"### {entry['title']}", "", f"Used by: {users}", "", "```text", entry["body"], "```", ""]
    args.output.write_text("\n".join(out), encoding="utf-8", newline="\n")
    print(f"{args.output}: {len(packages)} crates, {len(texts)} distinct license texts")


if __name__ == "__main__":
    main()
