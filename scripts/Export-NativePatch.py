"""Export reviewable patch stages and verify native prompts/key bindings are untouched."""

import argparse
import difflib
import hashlib
import json
from pathlib import Path
import stat
import zipfile

parser = argparse.ArgumentParser()
parser.add_argument("--archive", type=Path, required=True)
args = parser.parse_args()
project = Path(__file__).resolve().parents[1]
source = project / "upstream" / "codex-rust-v0.153.4"
output = project / "native-patch"
output.mkdir(exist_ok=True)
prefix = "codex-rust-v0.153.4/"
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
        diff = "".join(difflib.unified_diff(
            before.decode("utf-8").splitlines(keepends=True),
            after.decode("utf-8").splitlines(keepends=True),
            fromfile="a/" + name if name in baseline else "/dev/null",
            tofile="b/" + name,
        ))
        stages[stage].append(diff)
        changes.append({"path": name, "stage": stage, "sha256": hashlib.sha256(after).hexdigest()})
    for name, patches in stages.items():
        (output / name).write_text("".join(patches), encoding="utf-8", newline="\n")
    manifest = {
        "upstream_tag": "rust-v0.153.4",
        "upstream_commit": "042fb41b7c813ac7999105e886b2b7aa715b5081",
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
