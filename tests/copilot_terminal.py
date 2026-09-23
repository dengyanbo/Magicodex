r"""Acceptance for magicopilot around the real, unmodified GitHub Copilot CLI.

Copilot runs in BYOK offline mode against a local chat-completions fixture, with its own
temporary COPILOT_HOME, so no GitHub account, network or model quota is used. The harness plays
the terminal: ConPTY + pyte, answering colour and device queries like Windows Terminal.

    uv run --no-project --with pyte --with pywinpty --with psutil --with wcwidth --with pillow ^
        python -X utf8 tests\copilot_terminal.py copilot\target\release\magicopilot.exe [--windows-terminal] [--frames out]
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import psutil
import pyte
from wcwidth import wcswidth
from winpty import Backend, PtyProcess

PROMPT = "MAGIC_PROMPT 画一个会发光的法阵"
COMMENTARY = "我先看看目录结构，再开始施法。"
FINAL = ["MAGIC_FINAL ", "法阵已经", "完成展开。\n\n", "- 光芒从阵心", "流向下方\n"]
FIRST_DELAY = 6.0
FINAL_DELAY = 2.0

_orig_dsr = pyte.Screen.report_device_status
pyte.Screen.report_device_status = lambda self, *a, **k: None if k.get("private") else _orig_dsr(self, *a)
_orig_sgr = pyte.Screen.select_graphic_rendition
pyte.Screen.select_graphic_rendition = lambda self, *a, **k: None if k.get("private") else _orig_sgr(self, *a)

CAMPBELL = ["0c0c/0c0c/0c0c", "c5c5/0f0f/1f1f", "1313/a1a1/0e0e", "c1c1/9c9c/0000", "0000/3737/dada",
            "8888/1717/9898", "3a3a/9696/dddd", "cccc/cccc/cccc", "7676/7676/7676", "e7e7/4848/5656",
            "1616/c6c6/0c0c", "f9f9/f1f1/a5a5", "3b3b/7878/ffff", "b4b4/0000/9e9e", "6161/d6d6/d6d6",
            "f2f2/f2f2/f2f2"]


class Fixture(BaseHTTPRequestHandler):
    """An OpenAI-compatible chat-completions endpoint with one scripted conversation."""

    protocol_version = "HTTP/1.1"
    requests = []

    def log_message(self, *_args):
        pass

    def do_GET(self):
        body = json.dumps({"object": "list", "data": [{"id": "gpt-5.4", "object": "model"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        Fixture.requests.append(body)
        messages = body.get("messages", [])
        primary = bool(body.get("tools")) and any(
            PROMPT in json.dumps(m.get("content"), ensure_ascii=False) for m in messages if m.get("role") == "user")
        answered_tool = any(m.get("role") == "tool" for m in messages)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()

        def chunk(delta, finish=None):
            payload = {"id": "c1", "object": "chat.completion.chunk", "created": int(time.time()),
                       "model": body.get("model"), "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(f"data: {json.dumps(payload, ensure_ascii=False)}\n\n".encode())
            self.wfile.flush()

        chunk({"role": "assistant", "content": ""})
        if primary and not answered_tool:
            time.sleep(FIRST_DELAY)
            for part in [COMMENTARY[:6], COMMENTARY[6:]]:
                chunk({"content": part})
                time.sleep(0.2)
            chunk({"tool_calls": [{"index": 0, "id": "call_magic_1", "type": "function",
                                   "function": {"name": "glob", "arguments": ""}}]})
            chunk({"tool_calls": [{"index": 0, "function": {"arguments": json.dumps({"pattern": "*.md"})}}]})
            chunk({}, "tool_calls")
        elif primary:
            time.sleep(FINAL_DELAY)
            for part in FINAL:
                chunk({"content": part})
                time.sleep(0.25)
            chunk({}, "stop")
        else:
            chunk({"content": "Magic fixture"})
            chunk({}, "stop")
        usage = {"id": "c1", "object": "chat.completion.chunk", "created": int(time.time()), "model": body.get("model"),
                 "choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13}}
        self.wfile.write(f"data: {json.dumps(usage)}\n\ndata: [DONE]\n\n".encode())
        self.wfile.flush()
        self.close_connection = True


class Terminal:
    """ConPTY + pyte, answering the queries Windows Terminal would answer."""

    def __init__(self, argv, env, cwd, columns=120, lines=40):
        self.screen = pyte.Screen(columns, lines)
        self.stream = pyte.Stream(self.screen)
        self.lock = threading.Lock()
        self.raw = []
        self.process = PtyProcess.spawn(argv, env=env, cwd=str(cwd), dimensions=(lines, columns),
                                        backend=Backend.ConPTY)
        self.info = psutil.Process(self.process.pid)
        self.last_output = time.monotonic()
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def answer(self, data):
        for m in re.finditer(r"\x1b\[c|\x1b\[6n|\x1b\](10|11|4;(\d+));\?(?:\x07|\x1b\\)", data):
            s = m.group(0)
            if s == "\x1b[c":
                self.process.write("\x1b[?61;6;7;22;23;24;28;32;42c")
            elif s == "\x1b[6n":
                with self.lock:
                    y, x = self.screen.cursor.y, self.screen.cursor.x
                self.process.write(f"\x1b[{y + 1};{x + 1}R")
            elif m.group(1) == "10":
                self.process.write("\x1b]10;rgb:cccc/cccc/cccc\x1b\\")
            elif m.group(1) == "11":
                self.process.write("\x1b]11;rgb:0c0c/0c0c/0c0c\x1b\\")
            elif m.group(2):
                self.process.write(f"\x1b]4;{m.group(2)};rgb:{CAMPBELL[int(m.group(2))]}\x1b\\")

    def read(self):
        try:
            while True:
                data = self.process.read(65536)
                self.answer(data)
                with self.lock:
                    self.raw.append(data)
                    self.stream.feed(data)
                    self.last_output = time.monotonic()
        except EOFError:
            pass

    def settle(self):
        deadline = time.monotonic() + 1.0
        while time.monotonic() - self.last_output < 0.04 and time.monotonic() < deadline:
            time.sleep(0.005)

    def rows(self):
        self.settle()
        with self.lock:
            rows = []
            for y in range(self.screen.lines):
                line, x = [], 0
                while x < self.screen.columns:
                    # pyte may keep an empty stub after a partly overwritten wide character.
                    text = self.screen.buffer[y][x].data or " "
                    line.append(text)
                    x += max(1, wcswidth(text))
                rows.append("".join(line))
            return rows

    def text(self):
        return "\n".join(self.rows())

    def cells(self):
        self.settle()
        with self.lock:
            return [[(c.data, c.fg, c.bold, False, c.reverse) for c in
                     (self.screen.buffer[y][x] for x in range(self.screen.columns))]
                    for y in range(self.screen.lines)]

    def wait(self, predicate, timeout=30, what="condition"):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            rows = self.rows()
            if predicate(rows):
                return rows
            if not self.process.isalive():
                raise AssertionError(f"Process exited while waiting for {what}:\n" + "\n".join(rows))
            time.sleep(0.1)
        raise AssertionError(f"Timed out waiting for {what}:\n" + self.text())

    def dismiss_dialogs(self, settle=0.0):
        """Answers Copilot's first-run dialogs: trust this folder for the session, decline the
        offer to edit terminal key bindings. Waits up to `settle` seconds for one to appear."""
        deadline = time.monotonic() + settle
        for _ in range(80):
            text = self.text()
            if "Do you trust the files" in text:
                self.process.write("1")
            elif "Set up terminal for multi-line input" in text:
                self.process.write("\x1b")
            elif time.monotonic() >= deadline:
                return
            time.sleep(0.3)
        raise AssertionError("A first-run dialog did not close:\n" + self.text())

    def type(self, text, delay=0.03):
        self.dismiss_dialogs()
        for ch in text:
            self.process.write(ch)
            time.sleep(delay)

    def enter(self):
        time.sleep(0.4)
        # Never answer Copilot's terminal-setup dialog with Enter: it would edit real settings.
        self.dismiss_dialogs()
        self.process.write("\r")

    def resize(self, lines, columns):
        with self.lock:
            self.screen.resize(lines, columns)
        self.process.setwinsize(lines, columns)

    def close(self, timeout=15):
        deadline = time.monotonic() + timeout
        while self.process.isalive() and time.monotonic() < deadline:
            time.sleep(0.1)
        alive = self.process.isalive()
        if alive:
            for child in self.info.children(recursive=True):
                try:
                    child.kill()
                except psutil.Error:
                    pass
            self.process.terminate(force=True)
        return not alive


def input_style(rows):
    """Copilot draws its input box with half blocks or with rules, depending on the terminal."""
    if any(row.startswith("\u257b") for row in rows):
        return "halfblock"
    if any(row.startswith("\u2500\u2500\u2500") for row in rows):
        return "rules"
    return "unknown"


def braille_rows(rows):
    return [i for i, row in enumerate(rows) if sum("\u2801" <= c <= "\u28ff" for c in row) >= 2]


EDGES = set("─━═▄▀▔▁╻╹╭╮╰╯┌┐└┘")


def input_line(rows):
    """Text in Copilot's input box: the last prefixed row between two edge rows (the prompts in
    the conversation above are drawn the same way)."""
    edge = lambda row: bool(row.strip()) and set(row.strip()) <= EDGES
    for i in range(len(rows) - 2, 0, -1):
        line = rows[i].lstrip()
        if line[:1] in ("┃", "❯") and edge(rows[i - 1]) and edge(rows[i + 1]):
            return line[1:].strip()
    return None


def tab_row(rows):
    """Row of Copilot's tab bar, i.e. the height of the magic region above it."""
    return next((i for i, row in enumerate(rows) if "Sessions" in row and "Gists" in row), None)


def region_text(rows):
    top = tab_row(rows)
    return "\n".join(rows[:top]) if top is not None else ""


def copilot_env(home, port, windows_terminal):
    env = {k: v for k, v in os.environ.items() if not k.upper().startswith(("COPILOT_", "GH_", "GITHUB_", "WT_"))}
    env.update(COPILOT_HOME=str(home), COPILOT_OFFLINE="true", COPILOT_AUTO_UPDATE="false",
               COPILOT_PROVIDER_BASE_URL=f"http://127.0.0.1:{port}/v1", COPILOT_MODEL="gpt-5.4",
               NO_PROXY="127.0.0.1,localhost", no_proxy="127.0.0.1,localhost")
    # Copilot offers to edit Windows Terminal's settings on first run; keep any such edit away
    # from the real profile.
    local = home.parent / (home.name + "-localappdata")
    local.mkdir(exist_ok=True)
    env["LOCALAPPDATA"] = str(local)
    if windows_terminal:
        env["WT_SESSION"] = "00000000-0000-4000-8000-000000000000"
    if os.environ.get("MAGICOPILOT_LOG"):
        env["MAGICOPILOT_LOG"] = os.environ["MAGICOPILOT_LOG"]
    return env


def fresh_home(base, name, work):
    home = base / name
    if home.exists():
        shutil.rmtree(home)
    home.mkdir()
    (home / "config.json").write_text(json.dumps({
        "banner": "never", "showTipsOnStartup": False, "trustedFolders": [str(work)],
        "askedSetupTerminals": "windows-terminal", "updateTerminalTitle": True}), encoding="utf-8")
    return home


def primary_requests():
    return [r for r in Fixture.requests
            if r.get("tools") and any(PROMPT in json.dumps(m.get("content"), ensure_ascii=False)
                                      for m in r.get("messages", []) if m.get("role") == "user")]


def normalized_system(request):
    system = next(m["content"] for m in request["messages"] if m["role"] == "system")
    system = system if isinstance(system, str) else json.dumps(system, ensure_ascii=False)
    system = re.sub(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}", "<uuid>", system)
    # Each run has its own COPILOT_HOME, which the session folder path names.
    system = re.sub(r"[/\\]home(?:-direct)?[/\\]session-state", "/<home>/session-state", system)
    return re.sub(r"\d{4}-\d{2}-\d{2}[T ][0-9:.+-]+Z?", "<time>", system)


def run_turn(term, frames, prefix):
    term.type(PROMPT)
    term.enter()
    started = time.monotonic()
    time.sleep(1.2)
    early = term.rows()
    frames.append((f"{prefix}-02-charging", "提交后 1.2s", term.cells()))
    time.sleep(3.5)
    later = term.rows()
    frames.append((f"{prefix}-03-charged", "蓄力 4.7s", term.cells()))
    rows = term.wait(lambda r: all(c in region_text(r) for c in "目录结构施法"), timeout=20,
                     what="commentary orbiting the circle")
    frames.append((f"{prefix}-04-commentary", "中间回复环绕", term.cells()))
    rows = term.wait(lambda r: any("MAGIC_FINAL" in row for row in r), timeout=30, what="final answer")
    return started, early, later, rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("wrapper", type=Path)
    parser.add_argument("--windows-terminal", action="store_true")
    parser.add_argument("--frames", type=Path)
    parser.add_argument("--baseline", action="store_true", help="also run Copilot directly and compare requests")
    args = parser.parse_args()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    frames = []
    results = {}
    base = Path(tempfile.mkdtemp(prefix="magicopilot-e2e-"))
    work = base / "work"
    work.mkdir()
    (work / "README.md").write_text("# Magic fixture\n", encoding="utf-8")
    try:
        home = fresh_home(base, "home", work)
        env = copilot_env(home, server.server_port, args.windows_terminal)
        term = Terminal([str(args.wrapper.resolve())], env, work)
        try:
            rows = term.wait(lambda r: tab_row(r) is not None and braille_rows(r), timeout=40, what="Copilot below the idle circle")
            assert "Magic circle on" in rows[0], rows[0]
            term.dismiss_dialogs(settle=8.0)
            rows = term.rows()
            assert tab_row(rows) == 5, f"idle region should be 5 rows:\n" + "\n".join(rows)
            assert braille_rows(rows) and max(braille_rows(rows)) < 5, rows[:6]
            results["input_style"] = input_style(rows)
            frames.append(("01-idle", "待机：Copilot CLI 上方的小法阵", term.cells()))

            started, early, later, rows = run_turn(term, frames, "turn")
            assert tab_row(early) == 21 and tab_row(later) == 21, (tab_row(early), tab_row(later))
            grow = (len(braille_rows(early)), len(braille_rows(later)))
            width = lambda r: max(sum("\u2801" <= c <= "\u28ff" for c in row) for row in r[:21])
            assert width(later) > width(early), f"circle should grow: {grow}"
            assert all(c in region_text(later) for c in "MAGICPROT画法阵"), "the prompt orbits the circle"
            results["growth_rows"] = grow

            outlet = term.wait(lambda r: tab_row(r) == 24, timeout=8, what="outlet after the final answer")
            frames.append(("turn-05-outlet", "最终回复：法阵定格并向下释放", term.cells()))
            assert any("MAGIC_FINAL" in row for row in outlet[24:]), "answer is rendered by Copilot below"
            idle = term.wait(lambda r: tab_row(r) == 5, timeout=8, what="return to the idle circle")
            results["turn_seconds"] = round(time.monotonic() - started, 1)
            frames.append(("turn-06-complete", "回合完成，法阵收回", term.cells()))
            assert any("MAGIC_FINAL" in row for row in idle), "answer stays readable"
            if args.windows_terminal:
                raw = "".join(term.raw)
                assert "\x1b]9;4;" in raw, "progress indicator is forwarded to Windows Terminal"
                results["progress_forwarded"] = True

            before = len(Fixture.requests)
            # With the cursor moved back into the command, all of it is still erased.
            term.type("/magic off")
            term.process.write("\x1b[D" * 3)
            term.enter()
            rows = term.wait(lambda r: tab_row(r) == 1 and "Magic circle off" in r[0], timeout=5, what="off notice")
            rows = term.wait(lambda r: tab_row(r) == 0, timeout=6, what="Copilot at full height")
            assert not any("/magic" in row for row in rows[1:]), "the command left Copilot's input box"
            assert input_line(rows) == "", f"input box after /magic off: {input_line(rows)!r}"
            # So is a space typed after it.
            term.type("/magic 火 ")
            term.enter()
            rows = term.wait(lambda r: tab_row(r) == 5 and "fire 火" in r[0], timeout=5, what="fire style")
            assert input_line(rows) == "", f"input box after '/magic 火 ': {input_line(rows)!r}"
            results["typed_commands_erased"] = True
            frames.append(("07-fire-idle", "/magic 火", term.cells()))

            term.type("/magic list")
            term.enter()
            rows = term.wait(lambda r: any("Magic circle styles" in row for row in r), timeout=5, what="picker")
            assert tab_row(rows) == 21 and any("› 3. fire 火 (current)" in row for row in rows), "\n".join(rows[:21])
            term.process.write("\x1b[B")
            rows = term.wait(lambda r: any("› 4. water 水" in row for row in r), timeout=5, what="water highlighted")
            frames.append(("08-picker", "/magic list：移动即预览", term.cells()))
            term.process.write("\x1b")
            rows = term.wait(lambda r: tab_row(r) == 5 and not any("Magic circle styles" in row for row in r),
                             timeout=5, what="picker closed")
            term.type("/magic list")
            term.enter()
            term.wait(lambda r: any("Magic circle styles" in row for row in r), timeout=5, what="picker again")
            term.process.write("2")
            rows = term.wait(lambda r: tab_row(r) == 5 and "wind 风" in r[0], timeout=5, what="wind chosen by number")
            assert len(Fixture.requests) == before, "local /magic commands reached the model"

            # Mouse: a click on Copilot's own tab bar must reach it with the row shifted.
            rows = term.rows()
            top = tab_row(rows)
            def tab_bg(label):
                with term.lock:
                    return term.screen.buffer[top][rows[top].index(label) + 1].bg
            # Inactive tabs have a background of their own; compare with the selected one.
            selected = tab_bg("Current")
            assert tab_bg("Sessions") != selected, (selected, tab_bg("Sessions"))
            column = rows[top].index("Sessions") + 2
            term.process.write(f"\x1b[<0;{column};{top + 1}M")
            term.process.write(f"\x1b[<0;{column};{top + 1}m")
            term.wait(lambda r: tab_bg("Sessions") == selected, timeout=6, what="Sessions tab selected by a mouse click")
            results["mouse_click_reaches_copilot"] = True
            first = rows[top].index("Current") + 2
            term.process.write(f"\x1b[<0;{first};{top + 1}M\x1b[<0;{first};{top + 1}m")
            term.wait(lambda r: tab_bg("Current") == selected, timeout=6, what="Current tab selected again")
            time.sleep(0.5)

            # Too short for the picker: /magic list only explains, and keys still reach Copilot.
            term.resize(18, 120)
            term.wait(lambda r: tab_row(r) == 0, timeout=8, what="Copilot alone at 18 rows")
            term.type("/magic list")
            term.enter()
            rows = term.wait(lambda r: "窗口太矮" in r[0], timeout=5, what="too-short notice")
            assert not any("Magic circle styles" in row for row in rows), "\n".join(rows)
            term.type("x")
            term.wait(lambda r: input_line(r) == "x", timeout=5, what="keys reaching Copilot after /magic list")
            term.process.write("\x7f")
            term.wait(lambda r: input_line(r) == "", timeout=5, what="input box cleared")
            term.resize(40, 120)
            term.wait(lambda r: tab_row(r) == 5, timeout=8, what="idle circle at 40 rows again")
            results["short_terminal_picker_refused"] = True
            assert len(Fixture.requests) == before, "local /magic commands reached the model"

            term.type("/exit")
            term.enter()
            exited = term.close()
            assert exited, "Copilot's /exit ends the wrapper:\n" + "\n".join(term.rows())
            raw = "".join(term.raw)
            restored = raw.rfind("\x1b[?1049l")
            assert restored >= 0 and "\x1b[?1049h" not in raw[restored:], "terminal restored"
            results["exit_restores_terminal"] = True
            # Like a direct run, the exit summary with the resume command stays in the terminal.
            tail = raw[restored:]
            assert "Resume" in tail and "--resume=" in tail, "exit summary: " + repr(tail[:600])
            results["exit_summary_kept"] = True
        finally:
            if term.process.isalive():
                term.process.terminate(force=True)

        primaries = primary_requests()
        assert len(primaries) == 2, f"expected tool turn + final turn, got {len(primaries)}"
        user = next(m for m in primaries[0]["messages"] if m["role"] == "user")
        assert PROMPT in json.dumps(user["content"], ensure_ascii=False), "prompt reached the model unchanged"
        leaks = [json.dumps(r, ensure_ascii=False) for r in Fixture.requests
                 if re.search(r"/magic(?:\s|\"|$)", json.dumps(r, ensure_ascii=False))]
        assert not leaks, "/magic leaked: " + "; ".join(leak[:300] for leak in leaks)
        sessions = list((home / "session-state").iterdir())
        results["sessions"] = len(sessions)

        if args.baseline:
            wrapped_system = normalized_system(primaries[0])
            wrapped_tools = [t.get("function", {}).get("name") for t in primaries[0]["tools"]]
            Fixture.requests.clear()
            home = fresh_home(base, "home-direct", work)
            env = copilot_env(home, server.server_port, args.windows_terminal)
            copilot = shutil.which("copilot.cmd") or shutil.which("copilot")
            term = Terminal(["cmd.exe", "/d", "/c", copilot], env, work)
            try:
                term.wait(lambda r: tab_row(r) is not None, timeout=40, what="direct Copilot")
                term.dismiss_dialogs(settle=8.0)
                assert tab_row(term.rows()) == 0
                direct_style = input_style(term.rows())
                assert direct_style == results["input_style"], (
                    f"Copilot draws its input box as {results['input_style']} under the wrapper "
                    f"but as {direct_style} directly")
                results["input_box_matches_direct"] = True
                term.type(PROMPT)
                term.enter()
                term.wait(lambda r: any("MAGIC_FINAL" in row for row in r), timeout=40, what="direct final answer")
                time.sleep(1.5)
                term.type("/exit")
                term.enter()
                term.close()
            finally:
                if term.process.isalive():
                    term.process.terminate(force=True)
            direct = primary_requests()
            if normalized_system(direct[0]) != wrapped_system:
                import difflib
                diff = "\n".join(list(difflib.unified_diff(wrapped_system.splitlines(), normalized_system(direct[0]).splitlines(), lineterm="", n=0))[:40])
                raise AssertionError("default instructions differ under the wrapper:\n" + diff)
            assert [t.get("function", {}).get("name") for t in direct[0]["tools"]] == wrapped_tools
            results["default_instructions_unchanged"] = True
    finally:
        server.shutdown()
        server.server_close()
        if sys.exc_info()[0] is not None:
            write_frames(args.frames, frames)
        if os.environ.get("MAGICOPILOT_KEEP"):
            print("kept", base)
        else:
            shutil.rmtree(base, ignore_errors=True)
        write_frames(args.frames, frames)
    print(json.dumps({"magicopilot": "passed", **results}, ensure_ascii=False))


def write_frames(directory, frames):
    if not directory:
        return
    sys.path.insert(0, str(Path(__file__).parent))
    from render_frames import render
    directory.mkdir(parents=True, exist_ok=True)
    for name, title, cells in frames:
        render(cells, directory / f"{name}.png", f"magicopilot · {title}")
        (directory / f"{name}.txt").write_text(
            "\n".join("".join(cell[0] or "" for cell in row).rstrip() for row in cells), encoding="utf-8")


if __name__ == "__main__":
    main()
