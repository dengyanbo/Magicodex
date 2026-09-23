"""Real Windows ConPTY checks; never sends requests to a model."""

import argparse
import json
import os
import sys
from pathlib import Path
import threading
import time
import tempfile

import psutil
import pyte
from winpty import Backend, PtyProcess


class Terminal:
    def __init__(self, executable, arguments, cwd=None, env=None):
        self.screen = pyte.Screen(120, 40)
        self.stream = pyte.Stream(self.screen)
        self.lock = threading.Lock()
        self.output = []
        self.errors = []
        self.pty = PtyProcess.spawn(
            [str(executable), *arguments],
            cwd=str(cwd or executable.parent),
            env=env,
            dimensions=(40, 120),
            backend=Backend.ConPTY,
        )
        self.process = psutil.Process(self.pty.pid)
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()

    def _read(self):
        try:
            while True:
                text = self.pty.read(8192)
                with self.lock:
                    self.output.append(text)
                    self.stream.feed(text)
        except EOFError:
            pass
        except OSError as error:
            self.errors.append(str(error))

    def text(self):
        with self.lock:
            return "\n".join(self.screen.display)

    def captured(self):
        with self.lock:
            return "".join(self.output)

    def wait_for(self, text, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if text in self.text():
                return
            if not self.pty.isalive():
                self.reader.join(1)
                if text in self.text():
                    return
                raise AssertionError(f"Process exited waiting for {text!r}: {self.captured()}")
            time.sleep(0.025)
        raise AssertionError(f"Missing {text!r} on screen:\n{self.text()}")

    def write(self, text):
        self.pty.write(text)

    def resize(self, rows, columns):
        self.pty.setwinsize(rows, columns)
        with self.lock:
            self.screen.resize(rows, columns)
        time.sleep(0.2)

    def close(self):
        if self.pty.isalive():
            self.write("\x11")
            deadline = time.monotonic() + 5
            while self.pty.isalive() and time.monotonic() < deadline:
                time.sleep(0.025)
            if self.pty.isalive():
                self.pty.terminate(force=True)
                raise AssertionError("Owned terminal did not exit on Ctrl+Q")
        self.reader.join(3)
        if self.errors:
            raise AssertionError(f"PTY reader error: {self.errors}")
        if self.pty.exitstatus != 0:
            raise AssertionError(f"Exit status {self.pty.exitstatus}: {self.captured()}")
        if "\x1b[?1049l" not in self.captured():
            raise AssertionError("Alternate screen was not restored")


def cpu_ms(process):
    times = process.cpu_times()
    return (times.user + times.system) * 1000


def demo(executable, mode, capture):
    arguments = ["--demo"]
    if mode == "reduced":
        arguments.append("--reduced-motion")
    elif mode == "plain":
        arguments.append("--plain")
    term = Terminal(executable, arguments)
    try:
        term.wait_for("MAGICODEX")
        initial_cpu = cpu_ms(term.process)
        start = time.monotonic()
        peak = 0
        peak_rss = 0
        while time.monotonic() - start < 4:
            memory = term.process.memory_info()
            peak = max(peak, memory.private)
            peak_rss = max(peak_rss, memory.rss)
            time.sleep(0.1)
        active_cpu = cpu_ms(term.process) - initial_cpu
        term.wait_for("回合已完成")
        time.sleep(1.2)
        idle_length = len(term.captured())
        idle_cpu = cpu_ms(term.process)
        time.sleep(1.2)
        idle_cpu = cpu_ms(term.process) - idle_cpu
        idle_chars = len(term.captured()) - idle_length
        assert idle_chars == 0, f"Static terminal was redrawn: {idle_chars} characters"

        term.write("\x1bOS")
        term.wait_for("咒语已凝聚成形")
        term.write("\x1b[200~中文粘贴\n第二行\x1b[201~")
        term.wait_for("中文粘贴")
        assert "回合已完成" in term.text(), "Pasting submitted the input"
        term.resize(24, 80)
        term.wait_for("MAGICODEX")
        term.resize(40, 120)
        term.write("\r")
        term.wait_for("施法中")
        term.write("\x03")
        term.wait_for("已中断")
    finally:
        try:
            term.close()
        finally:
            if capture:
                capture.write_text(term.captured(), encoding="utf-8")
    return {
        "mode": mode,
        "peak_private_mib": round(peak / 1024 / 1024, 2),
        "peak_working_set_mib": round(peak_rss / 1024 / 1024, 2),
        "active_cpu_ms_over_4s": round(active_cpu, 2),
        "idle_cpu_ms_over_1_2s": round(idle_cpu, 2),
        "idle_terminal_characters": idle_chars,
        "unicode_paste_resize_interrupt_exit": "passed",
    }

def approvals(executable, fixture):
    with tempfile.TemporaryDirectory(prefix="magicodex-fixture-") as directory:
        marker = Path(directory) / "approved.txt"
        environment = dict(os.environ, MAGICODEX_FIXTURE_MARKER=str(marker))
        term = Terminal(executable, [
            "--backend", "official", "--codex", str(fixture), "--cwd", directory,
            "--reduced-motion",
        ], cwd=Path(directory), env=environment)
        try:
            term.wait_for("准备就绪")
            for prompt, accept in [
                ("COMMAND_DENY", False), ("COMMAND_ALLOW", True),
                ("FILE_DENY", False), ("FILE_ALLOW", True),
            ]:
                if marker.exists():
                    marker.unlink()
                term.write(prompt + "\r")
                term.wait_for("需要你的决定")
                assert not marker.exists(), "Tool executed before approval"
                term.write("\x1b[6~" * 4)
                term.write("y" if accept else "n")
                term.wait_for("TOOL_ACCEPTED" if accept else "TOOL_DECLINED")
                assert marker.exists() == accept, f"Permission was not enforced for {prompt}"
            term.write("QUESTION\r")
            term.wait_for("Choose a color")
            term.write("2\r")
            term.wait_for("synthetic secret")
            term.write(" synthetic_secret_731 ")
            time.sleep(0.2)
            assert "synthetic_secret_731" not in term.captured(), "Secret input was displayed"
            term.write("\r")
            term.wait_for("ANSWERS_OK")
            term.write("QUESTION\r")
            term.wait_for("Choose a color")
            term.write("\x1b")
            term.wait_for("已中断")
            term.write("UNKNOWN\r")
            term.wait_for("UNSUPPORTED_REJECTED")
            term.write("WAIT\r")
            term.wait_for("施法中")
            term.write("\x03")
            term.wait_for("已中断")
            term.write("\x0e")
            term.wait_for("准备就绪")
            term.write("NEW_SESSION\r")
            term.wait_for("FIXTURE_OK")
            term.write("\x13")
            term.wait_for("已按你的请求导出后端记录")
            deadline = time.monotonic() + 5
            while not list(Path(directory).glob("magicodex-*.json")) and time.monotonic() < deadline:
                time.sleep(0.025)
            exports = list(Path(directory).glob("magicodex-*.json"))
            assert len(exports) == 1, "Explicit history export did not complete"
            assert json.loads(exports[0].read_text(encoding="utf-8"))["thread"]["id"] == "fixture"
        finally:
            term.close()
    restore_mode(executable)
    return {"command_and_file_allow_deny": "passed", "secret_question_cancel_restart_export": "passed",
            "original_console_mode_and_code_page_restored": "passed"}


def restore_mode(executable):
    code = r"""
import ctypes, subprocess, sys
k = ctypes.WinDLL('kernel32', use_last_error=True)
k.GetStdHandle.argtypes = [ctypes.c_uint32]
k.GetStdHandle.restype = ctypes.c_void_p
k.GetConsoleMode.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_uint32)]
k.SetConsoleMode.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
h = k.GetStdHandle(ctypes.c_uint32(-10).value)
before = ctypes.c_uint32()
assert k.GetConsoleMode(h, ctypes.byref(before))
assert k.SetConsoleMode(h, before.value & ~4)
assert k.GetConsoleMode(h, ctypes.byref(before))
assert k.SetConsoleCP(437)
code_page = k.GetConsoleCP()
result = subprocess.run([sys.argv[1], '--demo', '--reduced-motion'])
after = ctypes.c_uint32()
assert k.GetConsoleMode(h, ctypes.byref(after))
assert before.value == after.value, (before.value, after.value)
assert code_page == k.GetConsoleCP()
assert result.returncode == 0
print('CONSOLE_STATE_RESTORED', flush=True)
"""
    term = Terminal(Path(sys.executable), ["-X", "utf8", "-c", code, str(executable)])
    try:
        term.wait_for("MAGICODEX")
        term.write("\x11")
        term.wait_for("CONSOLE_STATE_RESTORED")
    finally:
        term.close()

def live_tool(executable, backend, model):
    with tempfile.TemporaryDirectory(prefix="magicodex-live-") as directory:
        arguments = ["--backend", backend, "--cwd", directory, "--plain", "--reduced-motion"]
        if model:
            arguments.extend(["--model", model])
        term = Terminal(executable, arguments, cwd=Path(directory))
        owned = []
        peak_backend = 0
        try:
            term.wait_for("准备就绪", timeout=60)
            owned = term.process.children(recursive=True)
            peak_backend = sum(p.memory_info().private for p in owned if p.is_running())
            prompt = (
                "Harmless client integration test. Use a shell tool to run exactly "
                "Write-Output MAGICODEX_TOOL_731 in PowerShell. Do not read or modify files, "
                "access the network, or spawn agents. After the actual command succeeds, "
                "reply exactly TOOL_TEST_DONE. Do not claim execution without using the tool."
            )
            term.write("\x1b[200~" + prompt + "\x1b[201~")
            term.write("\r")
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                text = term.text()
                if "需要你的决定" in text:
                    raise AssertionError("Live test requires human approval; no automatic approval:\n" + text)
                if "回合已完成" in text:
                    break
                if "失败" in text or "已断开" in text:
                    raise AssertionError("Live tool run failed:\n" + text)
                time.sleep(0.05)
            else:
                raise AssertionError("Live tool run did not complete:\n" + term.text())
            term.write("\x13")
            term.wait_for("已按你的请求导出后端记录")
            deadline = time.monotonic() + 10
            while not list(Path(directory).glob("magicodex-*.json")) and time.monotonic() < deadline:
                time.sleep(0.05)
            exports = list(Path(directory).glob("magicodex-*.json"))
            assert len(exports) == 1, "Live history export failed:\n" + term.text()
            history = json.loads(exports[0].read_text(encoding="utf-8"))
            def items(value):
                if isinstance(value, dict):
                    if value.get("type") == "commandExecution":
                        yield value
                    for child in value.values():
                        yield from items(child)
                elif isinstance(value, list):
                    for child in value:
                        yield from items(child)
            commands = list(items(history))
            assert any(
                command.get("exitCode") == 0
                and "MAGICODEX_TOOL_731" in (command.get("aggregatedOutput") or "")
                for command in commands
            ), "No successful commandExecution evidence in exported backend history"
        finally:
            term.close()
        remaining = [p.pid for p in owned if p.is_running() and p.status() != psutil.STATUS_ZOMBIE]
        assert not remaining, f"Owned backend descendants remain: {remaining}"
        return {
            "backend": backend,
            "actual_command_execution_and_history_export": "passed",
            "initial_backend_private_mib": round(peak_backend / 1024 / 1024, 2),
            "owned_backend_processes_at_ready": len(owned),
            "remaining_owned_processes_after_exit": remaining,
        }


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("--mode", choices=["animated", "reduced", "plain"], default="animated")
    parser.add_argument("--capture", type=Path)
    parser.add_argument("--fixture", type=Path)
    parser.add_argument("--live", choices=["copilot", "official"])
    parser.add_argument("--model")
    args = parser.parse_args()
    if args.live:
        result = live_tool(args.executable.resolve(), args.live, args.model)
    elif args.fixture:
        result = approvals(args.executable.resolve(), args.fixture.resolve())
    else:
        result = demo(args.executable.resolve(), args.mode, args.capture)
    print(json.dumps(result, ensure_ascii=False))
