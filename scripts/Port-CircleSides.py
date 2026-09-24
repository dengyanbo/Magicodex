"""Copy magicopilot's circle sides into the Codex TUI source, adapting them to Codex.

The sides are drawn by the same code in both variants. After changing copilot\\src\\circle\\sides\\,
run this, format the Codex source (scripts\\Build-Native.ps1 -Action FormatRust) and export the
patch stage (scripts\\Export-NativePatch.py). --out writes the files elsewhere, for comparison.
"""

import argparse
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--out", type=Path, help="Directory receiving the files instead of codex-rs\\tui\\src")
args = parser.parse_args()
project = Path(__file__).resolve().parents[1]
source = project / "copilot" / "src" / "circle" / "sides"
tui = args.out or project / "upstream" / "codex-rust-v0.153.4" / "codex-rs" / "tui" / "src"
PATHS = [
    ("crate::circle::state::", "crate::magic_circle::"),
    ("crate::circle::style::", "crate::magic_style::"),
    ("crate::circle::canvas::", "crate::magic_canvas::"),
]
LIVE_VIEW = '''
/// The live circle above the composer, with its sides while a turn charges.
pub(crate) struct LiveView<'a> {
    pub(crate) circle: &'a MagicCircle,
    pub(crate) style: MagicStyle,
    pub(crate) animations_enabled: bool,
}

impl LiveView<'_> {
    fn view(&self) -> MagicView<'_> {
        MagicView {
            circle: self.circle,
            style: self.style,
            animations_enabled: self.animations_enabled,
            scene: MagicScene::Live,
        }
    }
}

impl Renderable for LiveView<'_> {
    fn desired_height(&self, width: u16) -> u16 {
        self.view().desired_height(width)
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let now = Instant::now();
        self.view().render_at(area, buf, now);
        SideView {
            circle: self.circle,
            style: self.style,
            animations: self.animations_enabled,
            outlet: None,
        }
        .render(area, buf, now);
    }
}
'''


def adapt(text):
    for old, new in PATHS:
        text = text.replace(old, new)
    return text.replace("\r\n", "\n")


def replace(text, old, new):
    assert old in text, f"magicopilot source changed; update {Path(__file__).name}: {old.strip()!r}"
    return text.replace(old, new, 1)


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))
    print("wrote", path)


for name in ["familiar", "pages", "particles", "pillars", "spells"]:
    text = adapt((source / f"{name}.rs").read_text(encoding="utf-8"))
    if name == "spells":
        # Copilot's session log reports skills and intents; Codex has no such events.
        for signature, indent in [("pub(crate) fn intent(", ""), ("pub(crate) fn tome(", "    "),
                                  ("pub(crate) fn intend(", "    ")]:
            kind = "skills" if "tome" in signature else "intents"
            text = replace(text, indent + signature,
                           f"{indent}// Copilot's session log reports {kind}; Codex has no such event.\n"
                           f"{indent}#[allow(dead_code)]\n{indent}{signature}")
    write(tui / "magic_sides" / f"{name}.rs", text)

module = adapt((source / "mod.rs").read_text(encoding="utf-8"))
head, marker, tests = module.partition("\n#[cfg(test)]\nmod tests {\n")
assert marker, "tests module not found"
# Codex keeps unit tests in a sibling file.
body = tests.rstrip().removesuffix("}").rstrip("\n")
tests_file = "\n".join(line[4:] if line.startswith("    ") else line for line in body.split("\n")) + "\n"
head = replace(head, "use crate::magic_circle::MagicCircle;\n",
               "use crate::magic_circle::MagicCircle;\nuse crate::magic_circle::MagicScene;\n"
               "use crate::magic_circle::MagicView;\n")
head = replace(head, "use crate::magic_style::MagicStyle;\n",
               "use crate::magic_style::MagicStyle;\nuse crate::render::renderable::Renderable;\n")
head = replace(head, "/// The answer's arrival, while the outlet shows.\n#[derive",
               "/// The answer's arrival, while the outlet shows. The live circle in Codex never shows one;\n"
               "/// its outlet goes to the transcript without the sides.\n"
               "#[cfg_attr(not(test), allow(dead_code))]\n#[derive")
write(tui / "magic_sides.rs",
      head.rstrip("\n") + "\n" + LIVE_VIEW + '\n#[cfg(test)]\n#[path = "magic_sides_tests.rs"]\nmod tests;\n')
write(tui / "magic_sides_tests.rs", tests_file)
