"""Native Codex UI acceptance using a local Responses fixture, without model charges."""

import argparse
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import subprocess
import shutil
import tempfile
import threading
import time

import pyte
import psutil
from winpty import Backend, PtyProcess
from wcwidth import wcswidth


class Fixture(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    requests = []
    primary_requests = []
    first_text = threading.Event()
    release_final = threading.Event()

    def log_message(self, *_args):
        pass

    def do_GET(self):
        payload = json.dumps({"object": "list", "data": [{"id": "gpt-5.5", "object": "model"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        self.requests.append(body)
        output_format = body.get("text", {}).get("format", {})
        auxiliary = output_format.get("type") == "json_schema"
        if not auxiliary:
            self.primary_requests.append(body)
        print(json.dumps({"request_path": self.path, "model": body.get("model"),
                          "auxiliary": auxiliary, "input_types": [
                              item.get("type", item.get("role")) for item in body.get("input", [])
                              if isinstance(item, dict)
                          ], "body_hash": hashlib.sha256(json.dumps(body, sort_keys=True).encode()).hexdigest()[:12]}), flush=True)
        number = len(self.primary_requests)
        response_id = f"resp_native_{len(self.requests)}"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()

        def emit(kind, **data):
            payload = json.dumps({"type": kind, **data}, ensure_ascii=False)
            self.wfile.write(f"event: {kind}\ndata: {payload}\n\n".encode())
            self.wfile.flush()

        emit("response.created", response={"id": response_id, "object": "response", "status": "in_progress",
                                           "model": body["model"], "created_at": int(time.time())})
        if number == 2 and not auxiliary:
            time.sleep(7)
        items = []
        texts = [("commentary", "Inspecting the constellation"), ("final_answer", f"NATIVE_RESPONSE_{number}")]
        if auxiliary:
            schema = output_format["schema"]
            def example(part):
                if "$ref" in part:
                    target = schema
                    for key in part["$ref"].removeprefix("#/").split("/"):
                        target = target[key]
                    return example(target)
                if "const" in part:
                    return part["const"]
                if "enum" in part:
                    return part["enum"][0]
                if "anyOf" in part:
                    return example(part["anyOf"][0])
                kind = part.get("type")
                if kind == "object":
                    return {name: example(child) for name, child in part.get("properties", {}).items()}
                if kind == "array":
                    return [example(part["items"]) for _ in range(part.get("minItems", 0))]
                if kind in ["integer", "number"]:
                    return part.get("minimum", 0)
                if kind == "boolean":
                    return False
                if kind == "null":
                    return None
                return "Native fixture"
            texts = [("final_answer", json.dumps(example(schema)))]
        for index, (phase, text) in enumerate(texts):
            pouring = number == 2 and phase == "final_answer" and not auxiliary
            if pouring:
                text = "POUR_FIRST_青蓝星环 · NATIVE_RESPONSE_2"
            item = {"id": f"msg_{number}_{index}", "type": "message", "role": "assistant",
                    "phase": phase, "status": "in_progress", "content": []}
            emit("response.output_item.added", output_index=index, item=item)
            part = {"type": "output_text", "text": "", "annotations": []}
            emit("response.content_part.added", item_id=item["id"], output_index=index, content_index=0, part=part)
            first_chunk = "POUR_FIRST_青蓝星环" if pouring else text
            emit("response.output_text.delta", item_id=item["id"], output_index=index, content_index=0, delta=first_chunk)
            if pouring:
                if not self.release_final.wait(20):
                    raise AssertionError("Terminal never observed the partial downward answer")
                emit("response.output_text.delta", item_id=item["id"], output_index=index,
                     content_index=0, delta=text[len(first_chunk):])
            if number == 2 and index == 0 and not auxiliary:
                self.first_text.set()
                time.sleep(3)
            part["text"] = text
            emit("response.output_text.done", item_id=item["id"], output_index=index, content_index=0, text=text)
            emit("response.content_part.done", item_id=item["id"], output_index=index, content_index=0, part=part)
            item.update(status="completed", content=[part])
            emit("response.output_item.done", output_index=index, item=item)
            items.append(item)
        emit("response.completed", response={
            "id": response_id, "object": "response", "status": "completed", "output": items,
            "model": body["model"], "created_at": int(time.time()), "end_turn": True,
            "usage": {"input_tokens": 20, "output_tokens": 10, "total_tokens": 30},
        })
        self.close_connection = True


class NativeTerminal:
    def __init__(self, binary, env, cwd, arguments=None):
        self.screen = pyte.Screen(120, 45)
        self.lock = threading.Lock()
        self.capture = []
        self.process = PtyProcess.spawn(
            [str(binary), *(arguments if arguments is not None else ["--no-alt-screen", "-C", str(cwd)])],
            env=env, cwd=str(cwd), dimensions=(45, 120), backend=Backend.ConPTY,
        )
        self.process_info = psutil.Process(self.process.pid)
        self.screen.write_process_input = self.process.write
        self.stream = pyte.Stream(self.screen)
        self.last_output = time.monotonic()
        self.reader = threading.Thread(target=self.read, daemon=True)
        self.reader.start()

    def read(self):
        try:
            while True:
                chunk = self.process.read(8192)
                with self.lock:
                    self.capture.append(chunk)
                    self.stream.feed(chunk)
                    self.last_output = time.monotonic()
        except EOFError:
            pass

    def settle(self):
        # pyte ignores synchronized output, so wait for a quiet gap instead of reading half a frame.
        deadline = time.monotonic() + 1.0
        while time.monotonic() - self.last_output < 0.03 and time.monotonic() < deadline:
            time.sleep(0.005)

    def text(self):
        self.settle()
        with self.lock:
            return "\n".join(self.display_rows())

    def display_rows(self):
        rows = []
        for y in range(self.screen.lines):
            row = []
            x = 0
            while x < self.screen.columns:
                # pyte may retain an empty wide-character stub after a partial redraw.
                text = self.screen.buffer[y][x].data or " "
                row.append(text)
                x += max(1, wcswidth(text))
            rows.append("".join(row))
        return rows

    def wait(self, text, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if text in self.text():
                return
            if not self.process.isalive():
                self.reader.join(1)
                raise AssertionError(f"Native process exited:\n{self.text()}")
            time.sleep(0.05)
        raise AssertionError(f"Missing {text!r}:\n{self.text()}")

    def wait_absent(self, text, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if text not in self.text():
                return
            time.sleep(0.05)
        raise AssertionError(f"Still showing {text!r}:\n{self.text()}")

    def submit(self, text):
        self.process.write("\x1b[200~" + text + "\x1b[201~")
        time.sleep(0.25)
        self.process.write("\r")

    def control(self, command, confirmation, timeout=60):
        self.submit(command)
        deadline = time.monotonic() + timeout
        next_submit = time.monotonic() + 2
        while time.monotonic() < deadline:
            screen = self.text()
            if confirmation in screen:
                return
            if time.monotonic() >= next_submit and any(
                line.strip() == f"› {command}" for line in screen.splitlines()
            ):
                self.process.write("\r")
                next_submit = time.monotonic() + 2
            time.sleep(0.1)
        raise AssertionError(f"Local command did not complete: {command}\n{self.text()}")

    def extent(self):
        self.settle()
        with self.lock:
            dots = [(x, y) for y, line in enumerate(self.display_rows())
                    for x, char in enumerate(line) if 28 <= x <= 92 and "\u2801" <= char <= "\u28ff"]
        if not dots:
            return (0, 0)
        xs, ys = zip(*dots)
        return max(xs) - min(xs) + 1, max(ys) - min(ys) + 1

    def wait_extent(self, predicate, timeout=5):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            extent = self.extent()
            if predicate(extent):
                return extent
            time.sleep(0.05)
        raise AssertionError(f"Unexpected circle extent: {self.extent()}\n{self.text()}")

    def close(self):
        children = self.process_info.children(recursive=True) if self.process.isalive() else []
        if self.process.isalive():
            self.process.write("\x04")
            deadline = time.monotonic() + 8
            while self.process.isalive() and time.monotonic() < deadline:
                time.sleep(0.05)
            if self.process.isalive():
                self.process.write("\x03")
                time.sleep(0.2)
                if self.process.isalive():
                    self.process.write("\x03")
                    time.sleep(1)
            if self.process.isalive():
                self.process.terminate(force=True)
                raise AssertionError("Native quit shortcuts did not exit")
        self.reader.join(2)
        remaining = [str(child.pid) for child in children if child.is_running()]
        if remaining:
            subprocess.run(["powershell.exe", "-NoProfile", "-Command",
                            "Stop-Process -Id " + ",".join(remaining) + " -ErrorAction Stop"], check=True)
            psutil.wait_procs(children, timeout=5)


def outlet_above(rows, needle):
    """Return the answer row and the lowest row of the circle and light cone above it."""
    answer = next(i for i, line in enumerate(rows) if needle in line)
    braille = [i for i, line in enumerate(rows[:answer]) if any("\u2801" <= char <= "\u28ff" for char in line)]
    assert len(braille) >= 8, "No magic outlet above the answer:\n" + "\n".join(rows)
    assert answer - braille[-1] <= 3, "The light cone should lead directly into the answer:\n" + "\n".join(rows)
    return answer, braille[-1]


def run(binary, baseline=False, windows_terminal=False):
    Fixture.requests = []
    Fixture.primary_requests = []
    Fixture.first_text.clear()
    Fixture.release_final.clear()
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix="magicodex-native-ui-") as directory:
            base = Path(directory)
            home, work = base / "home", base / "work"
            home.mkdir()
            work.mkdir()
            (work / ".codex").mkdir()
            config = f'''
model = "gpt-5.5"
model_provider = "magic_test"
approval_policy = "on-request"
sandbox_mode = "read-only"
web_search = "disabled"
[features]
enable_request_compression = false
multi_agent = false
plugins = false
[model_providers.magic_test]
name = "Local native UI fixture"
base_url = "http://127.0.0.1:{server.server_port}/v1"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
[windows]
sandbox = "unelevated"
[projects.{json.dumps(str(work))}]
trust_level = "trusted"
'''
            (home / "config.toml").write_text(config, encoding="utf-8")
            env = dict(os.environ, CODEX_HOME=str(home), NO_PROXY="127.0.0.1,localhost,::1",
                       no_proxy="127.0.0.1,localhost,::1")
            for key in ["OPENAI_API_KEY", "CODEX_API_KEY", "CODEX_COPILOT_PROXY_TOKEN", "WT_SESSION"]:
                env.pop(key, None)
            if windows_terminal:
                # Codex picks its Windows Terminal scrollback strategy from WT_SESSION.
                env["WT_SESSION"] = "00000000-0000-4000-8000-000000000000"
            terminal = NativeTerminal(binary, env, work)
            try:
                terminal.wait("Codex")
                time.sleep(1)
                assert terminal.extent() == (0, 0), "Magic should be opt-in"
                if baseline:
                    terminal.submit("NATIVE_BASELINE_青蓝星环")
                    terminal.wait("NATIVE_RESPONSE_1")
                    time.sleep(0.75)
                    assert len(Fixture.primary_requests) == 1
                    assert isinstance(Fixture.primary_requests[0].get("instructions"), str)
                    print(json.dumps({"unmodified_native_fixture": "passed", "model_requests": 1}))
                    return
                terminal.submit("/magic list")
                terminal.wait("Magic circle styles")
                terminal.process.write("\x1b[B")
                terminal.wait("› 2. wind 风")
                terminal.process.write("\x1b")
                terminal.wait_absent("Magic circle styles")
                assert terminal.extent() == (0, 0), "Previewing a style must not turn the circle on"
                assert len(Fixture.primary_requests) == 0, "Local command invoked a model"
                prompt = "NATIVE_PROMPT_青蓝星环"
                terminal.submit(prompt)
                terminal.wait("NATIVE_RESPONSE_1")
                terminal.submit("/magic on")
                terminal.wait("Magic circle on")
                idle = terminal.wait_extent(lambda size: size[0] > 0)
                terminal.submit(prompt)
                time.sleep(1)
                early = terminal.extent()
                time.sleep(4)
                later = terminal.extent()
                assert later[0] > early[0] >= idle[0], (idle, early, later)
                assert later[1] > idle[1], (idle, early, later)
                assert Fixture.first_text.wait(8), "No assistant reply"
                terminal.wait("POUR_FIRST_青蓝星环")
                outlet_above(terminal.text().splitlines(), "POUR_FIRST_青蓝星环")
                assert "NATIVE_RESPONSE_2" not in terminal.text(), "Only the first chunk should exist yet"
                terminal.submit("/magic off")
                terminal.wait_extent(lambda size: size == (0, 0))
                terminal.submit("/magic on")
                terminal.wait("POUR_FIRST_青蓝星环")
                with terminal.lock:
                    terminal.screen.resize(lines=45, columns=90)
                    terminal.process.setwinsize(45, 90)
                time.sleep(0.5)
                outlet_above(terminal.text().splitlines(), "POUR_FIRST_青蓝星环")
                Fixture.release_final.set()
                terminal.wait("NATIVE_RESPONSE_2")
                time.sleep(0.3)
                completed = terminal.text().splitlines()
                answer_row, _ = outlet_above(completed, "POUR_FIRST_青蓝星环")
                assert sum("POUR_FIRST_青蓝星环" in line for line in completed) == 1, "Provisional answer was duplicated"
                assert not any("\u2801" <= char <= "\u28ff"
                               for line in completed[answer_row + 1:] for char in line[28:93]), "A new circle appeared below the answer"
                terminal.submit("/magic off")
                terminal.wait("Magic circle off")
                terminal.wait_extent(lambda size: size == (0, 0))
                terminal.submit("/magic list")
                terminal.wait("Magic circle styles")
                terminal.process.write("\x1b[B\x1b[B")
                terminal.wait("› 3. fire 火")
                terminal.process.write("\r")
                terminal.wait("Magic circle on · fire 火")
                terminal.wait_extent(lambda size: size[0] > 0)
                terminal.submit(prompt)
                terminal.wait("NATIVE_RESPONSE_3")
                time.sleep(0.3)
                outlet_above(terminal.text().splitlines(), "NATIVE_RESPONSE_3")
                terminal.submit("/magic 雷")
                terminal.wait("Magic circle on · thunder 雷")
                terminal.submit("/magic off")
                terminal.wait("Magic circle off · thunder 雷")
                terminal.wait_extent(lambda size: size == (0, 0))
                assert len(Fixture.primary_requests) == 3, "Magic commands reached inference"
                assert all(request["instructions"] == Fixture.primary_requests[0]["instructions"]
                           for request in Fixture.primary_requests), "Default instructions changed"
                assert all("/magic " not in json.dumps(body) for body in Fixture.requests)
                print(json.dumps({"native_commands": "passed", "growth": [idle, early, later],
                                  "model_requests": 3, "default_instructions_unchanged": True,
                                  "style_picker_preview_cancel_select": True, "styled_outlet": "fire",
                                  "partial_and_complete_reply_below_outlet": True,
                                  "stream_toggle_and_resize": True}))
            finally:
                Fixture.release_final.set()
                terminal.close()
    finally:
        server.shutdown()
        server.server_close()

def live_bridge(project):
    node = shutil.which("node")
    if not node:
        raise RuntimeError("Node is required by the existing bridge")
    with tempfile.TemporaryDirectory(prefix="magicodex-native-bridge-") as directory:
        work = Path(directory)
        terminal = NativeTerminal(Path(node), dict(os.environ), work, [
            str(project / "scripts" / "Start-NativeBridge.mjs"),
            "--no-alt-screen", "-C", str(work),
        ])
        try:
            deadline = time.monotonic() + 60
            skipped_update = False
            while time.monotonic() < deadline:
                screen = terminal.text()
                if "Update available!" in screen or "Update now (runs" in screen:
                    if not skipped_update:
                        terminal.process.write("\x03")
                        skipped_update = True
                elif ("Do you trust" in screen or "trust this" in screen.lower()
                      or "Yes, I trust" in screen) and work.name in screen:
                    terminal.process.write("\r")
                    time.sleep(0.3)
                elif "for shortcuts" in screen or "Ask Codex" in screen:
                    break
                time.sleep(0.1)
            else:
                raise AssertionError("Native composer was not ready; no unrecognized prompt was accepted:\n" + terminal.text())
            terminal.control("/magic on", "Magic circle on")
            terminal.wait_extent(lambda size: size[0] > 0)
            terminal.submit("Do not use tools. Reply only NATIVE_BRIDGE_READY_731.")
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                if any(line.strip().lstrip("• ").strip() == "NATIVE_BRIDGE_READY_731"
                       for line in terminal.text().splitlines()):
                    break
                time.sleep(0.1)
            else:
                raise AssertionError("No actual assistant response from the native bridge:\n" + terminal.text())
            time.sleep(1)
            terminal.control("/magic off", "Magic circle off")
            terminal.wait_extent(lambda size: size == (0, 0))
            print(json.dumps({"native_copilot_bridge": "passed", "magic_on_off": "passed"}))
        finally:
            terminal.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--baseline", action="store_true")
    parser.add_argument("--live-bridge", action="store_true")
    parser.add_argument("--windows-terminal", action="store_true",
                        help="Exercise the Windows Terminal scrollback strategy (WT_SESSION)")
    args = parser.parse_args()
    if args.live_bridge:
        live_bridge(args.binary.resolve())
    else:
        run(args.binary.resolve(), args.baseline, args.windows_terminal)
