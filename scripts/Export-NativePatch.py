"""Export reviewable patch stages and verify native prompts/key bindings are untouched."""

import argparse
import difflib
import hashlib
import json
from pathlib import Path
import stat
import subprocess
import tempfile
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("--archive", type=Path, required=True)
parser.add_argument("--base-ref", help="Keep existing patch stages and export subsequent UI work separately")
parser.add_argument("--keep-stage", type=Path, action="append", default=[],
                    help="Uncommitted stage kept verbatim after --base-ref, in order; repeat for several. "
                         "Newer work is diffed against the last kept stage that touched each file")
parser.add_argument("--stage-name", default="0003-downward-reply.patch",
                    help="Patch file receiving work after --base-ref and --keep-stage")
args = parser.parse_args()
if args.keep_stage and not args.base_ref:
    parser.error("--keep-stage requires --base-ref")
project = Path(__file__).resolve().parents[1]
source = project / "upstream" / "codex-rust-v0.153.4"
output = project / "native-patch"
output.mkdir(exist_ok=True)
prefix = "codex-rust-v0.153.4/"
native_prefix = "upstream/codex-rust-v0.153.4/"
base_paths = set()
if args.base_ref:
    subprocess.run(["git", "-C", str(project), "rev-parse", "--verify", args.base_ref], check=True, stdout=subprocess.DEVNULL)
    base_paths = set(subprocess.check_output(
        ["git", "-C", str(project), "ls-tree", "-r", "--name-only", args.base_ref, "--", native_prefix]
    ).decode().splitlines())


def at_base(name):
    tracked = native_prefix + name
    if tracked not in base_paths:
        return b""
    return subprocess.check_output(["git", "-C", str(project), "show", f"{args.base_ref}:{tracked}"])


def replay_kept_stage(patch, state):
    """Contents of every file touched by `patch` once it is applied after the earlier kept stages."""
    touched = [line[len("+++ b/"):].strip() for line in patch.read_text(encoding="utf-8").splitlines()
               if line.startswith("+++ b/")]
    with tempfile.TemporaryDirectory(prefix="magicodex-stage-") as directory:
        root = Path(directory)
        for name in touched:
            content = state[name] if name in state else at_base(name)
            if content:
                target = root.joinpath(*name.split("/"))
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_bytes(content)
        subprocess.run(["git", "-c", "core.autocrlf=false", "apply", "--whitespace=nowarn", str(patch.resolve())],
                       cwd=root, check=True)
        return {name: root.joinpath(*name.split("/")).read_bytes() for name in touched}
protected = (
    "codex-rs/core/", "codex-rs/protocol/", "codex-rs/prompts/",
    "codex-rs/models-manager/", "codex-rs/cli/",
    "codex-rs/tui/src/bottom_pane/", "codex-rs/tui/src/keymap",
    "codex-rs/tui/src/ui_consts.rs", "codex-rs/tui/src/tooltips.rs",
    "codex-rs/tui/src/cli.rs", "codex-rs/tui/src/terminal_visualization_instructions.rs",
)
with zipfile.ZipFile(args.archive) as archive:
    baseline = {
        i.filename[len(prefix):]: archive.read(i)
        for i in archive.infolist()
        if i.filename.startswith(prefix) and not i.is_dir()
        and not stat.S_ISLNK(i.external_attr >> 16)
    }
    checked = 0
    tests_only = {
        "codex-rs/tui/src/bottom_pane/command_popup.rs",
        "codex-rs/tui/src/bottom_pane/slash_commands.rs",
    }
    for name, original in baseline.items():
        if name.startswith(protected) and not name.endswith(".snap"):
            current = source.joinpath(*name.split("/")).read_bytes()
            if name in tests_only:
                assert current.split(b"#[cfg(test)]")[0] == original.split(b"#[cfg(test)]")[0], f"Protected implementation changed: {name}"
            else:
                assert current == original, f"Protected native behavior changed: {name}"
            checked += 1
    stages = {"0001-release-lock-alignment.patch": [], "0002-native-magic-tui.patch": []}
    kept = {}
    if args.base_ref:
        for stage in stages:
            stages[stage].append(subprocess.check_output(
                ["git", "-C", str(project), "show", f"{args.base_ref}:native-patch/{stage}"]
            ).decode("utf-8"))
        owners = {}
        for patch in args.keep_stage:
            # Decode without newline translation so the kept stage is rewritten byte for byte.
            stages[patch.name] = [patch.read_bytes().decode("utf-8")]
            for name, content in replay_kept_stage(patch, kept).items():
                kept[name] = content
                owners[name] = patch.name
        stages[args.stage_name] = []
    changes = []
    candidates = {"codex-rs/Cargo.lock", "MODULE.bazel.lock"}
    for file in (source / "codex-rs" / "tui").rglob("*"):
        if file.is_file() and file.suffix in {".rs", ".snap"}:
            candidates.add(file.relative_to(source).as_posix())
    for name in sorted(candidates):
        file = source.joinpath(*name.split("/"))
        before = baseline.get(name, b"")
        after = file.read_bytes()
        if before == after:
            continue
        release_fixture = (name.endswith(".snap") and name in baseline) or name.endswith("/chatwidget/rendering_tests.rs")
        stage = "0002-native-magic-tui.patch" if name.startswith("codex-rs/tui/") and not release_fixture else "0001-release-lock-alignment.patch"
        if args.base_ref:
            before = at_base(name)
            if before == after:
                changes.append({"path": name, "stage": stage, "sha256": hashlib.sha256(after).hexdigest()})
                continue
            if name in kept:
                if kept[name] == after:
                    changes.append({"path": name, "stage": owners[name],
                                    "sha256": hashlib.sha256(after).hexdigest()})
                    continue
                before = kept[name]
            stage = args.stage_name
        diff = "".join(difflib.unified_diff(
            before.decode("utf-8").splitlines(keepends=True),
            after.decode("utf-8").splitlines(keepends=True),
            fromfile="a/" + name if before else "/dev/null",
            tofile="b/" + name,
        ))
        stages[stage].append(diff)
        changes.append({"path": name, "stage": stage, "sha256": hashlib.sha256(after).hexdigest()})
    for name, patches in stages.items():
        (output / name).write_text("".join(patches), encoding="utf-8", newline="\n")
    manifest = {
        "upstream_tag": "rust-v0.153.4",
        "upstream_commit": "042fb41b7c813ac7999105e886b2b7aa715b5081",
        "native_magic_base_ref": args.base_ref,
        "kept_stages": [patch.name for patch in args.keep_stage],
        "protected_files_unchanged": checked,
        "windows_migration_preparation": {
            "script": "scripts/Prepare-NativeWindows.py",
            "line_endings": "CRLF",
            "sql_semantics": "unchanged",
        },
        "changes": changes,
    }
    (output / "manifest.json").write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"protected_files_unchanged": checked, "changed_files": len(changes)}))
