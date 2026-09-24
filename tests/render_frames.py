r"""Render the magic circle's stages from the real native CLI to PNG, without model calls.

Drives the patched codex.exe through ConPTY with a local Responses fixture, records pyte cells
(character, colour, bold, faint) once each frame has finished drawing, and paints an
approximation of Windows Terminal's Campbell scheme. These are buffer renderings, not
screenshots; fonts and exact colours in a real terminal differ.

    uv run --no-project --with pillow --with pyte --with pywinpty --with psutil --with wcwidth ^
        python -X utf8 tests\render_frames.py native\codex.exe out --windows-terminal --style fire
"""

import argparse
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import tempfile
import threading
import time

import psutil
import pyte
from PIL import Image, ImageDraw, ImageFont
from winpty import Backend, PtyProcess

PROMPT = "帮我把 terminal 渲染改成魔法阵，等待模型时逐渐展开 Arcane circle"
COMMENTARY = ["我先检查", "渲染循环与", "流式输出的", "提交顺序。"]
ANSWER = [
    "法阵已经", "完成展开，", "结果如下：\n\n",
    "- 渲染循环", "改为按需重绘\n",
    "- 文本沿圆环", "排布\n",
    "- 最终回复从", "法阵下方流出\n\n",
    "所有改动都保留了", "原生快捷键。",
]
WAIT_BEFORE_OUTPUT = 9.0


class Fixture(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    primary = 0
    events = []

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
        output_format = body.get("text", {}).get("format", {})
        auxiliary = output_format.get("type") == "json_schema"
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()

        def emit(kind, **data):
            payload = json.dumps({"type": kind, **data}, ensure_ascii=False)
            self.wfile.write(f"event: {kind}\ndata: {payload}\n\n".encode())
            self.wfile.flush()

        response_id = f"resp_visual_{time.monotonic_ns()}"
        emit("response.created", response={"id": response_id, "object": "response", "status": "in_progress",
                                           "model": body["model"], "created_at": int(time.time())})
        items = []
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
                return "Visual fixture"

            plan = [("final_answer", [json.dumps(example(schema))], 0.0)]
        else:
            Fixture.primary += 1
            Fixture.events.append(("request", time.monotonic()))
            time.sleep(WAIT_BEFORE_OUTPUT)
            plan = [("commentary", COMMENTARY, 0.45), ("final_answer", ANSWER, 0.45)]
        for index, (phase, chunks, pause) in enumerate(plan):
            item_id = f"msg_{response_id}_{index}"
            item = {"id": item_id, "type": "message", "role": "assistant", "phase": phase,
                    "status": "in_progress", "content": []}
            emit("response.output_item.added", output_index=index, item=item)
            part = {"type": "output_text", "text": "", "annotations": []}
            emit("response.content_part.added", item_id=item_id, output_index=index, content_index=0, part=part)
            if not auxiliary:
                Fixture.events.append((phase, time.monotonic()))
            for chunk in chunks:
                emit("response.output_text.delta", item_id=item_id, output_index=index, content_index=0, delta=chunk)
                time.sleep(pause)
            text = "".join(chunks)
            part["text"] = text
            emit("response.output_text.done", item_id=item_id, output_index=index, content_index=0, text=text)
            emit("response.content_part.done", item_id=item_id, output_index=index, content_index=0, part=part)
            item.update(status="completed", content=[part])
            emit("response.output_item.done", output_index=index, item=item)
            items.append(item)
            if phase == "commentary":
                time.sleep(3.5)
        emit("response.completed", response={
            "id": response_id, "object": "response", "status": "completed", "output": items,
            "model": body["model"], "created_at": int(time.time()), "end_turn": True,
            "usage": {"input_tokens": 20, "output_tokens": 10, "total_tokens": 30},
        })
        if not auxiliary:
            Fixture.events.append(("completed", time.monotonic()))
        self.close_connection = True


class StyledScreen(pyte.Screen):
    """pyte ignores SGR 2 (faint); record it in the unused blink attribute."""

    def select_graphic_rendition(self, *attrs, **kwargs):
        mapped = []
        values = list(attrs)
        index = 0
        while index < len(values):
            value = values[index]
            if value in (38, 48) and index + 1 < len(values):
                width = 3 if values[index + 1] == 5 else 5
                mapped.extend(values[index:index + width])
                index += width
                continue
            if value == 2:
                mapped.append(5)
            elif value == 22:
                mapped.extend([22, 25])
            else:
                mapped.append(value)
            index += 1
        super().select_graphic_rendition(*mapped, **kwargs)


class Terminal:
    def __init__(self, binary, env, cwd, columns, lines):
        self.screen = StyledScreen(columns, lines)
        self.lock = threading.Lock()
        self.process = PtyProcess.spawn([str(binary), "--no-alt-screen", "-C", str(cwd)], env=env,
                                        cwd=str(cwd), dimensions=(lines, columns), backend=Backend.ConPTY)
        self.info = psutil.Process(self.process.pid)
        self.screen.write_process_input = self.process.write
        self.stream = pyte.Stream(self.screen)
        self.last_output = time.monotonic()
        threading.Thread(target=self.read, daemon=True).start()

    def read(self):
        try:
            while True:
                chunk = self.process.read(8192)
                with self.lock:
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
        with self.lock:
            # Wide characters leave an empty stub cell; joining raw data keeps CJK text contiguous.
            return "\n".join("".join(self.screen.buffer[y][x].data for x in range(self.screen.columns))
                             for y in range(self.screen.lines))

    def wait(self, needle, timeout=40):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if needle in self.text():
                return
            time.sleep(0.05)
        raise AssertionError(f"Missing {needle!r}\n{self.text()}")

    def submit(self, text):
        self.process.write("\x1b[200~" + text + "\x1b[201~")
        time.sleep(0.3)
        self.process.write("\r")

    def cells(self):
        self.settle()
        with self.lock:
            return [[(c.data, c.fg, c.bold, c.blink, c.reverse) for c in
                     (self.screen.buffer[y][x] for x in range(self.screen.columns))]
                    for y in range(self.screen.lines)]

    def close(self):
        children = self.info.children(recursive=True) if self.process.isalive() else []
        if self.process.isalive():
            self.process.write("\x04")
            deadline = time.monotonic() + 8
            while self.process.isalive() and time.monotonic() < deadline:
                time.sleep(0.05)
            if self.process.isalive():
                self.process.write("\x03")
                time.sleep(0.5)
            if self.process.isalive():
                self.process.terminate(force=True)
        remaining = [str(child.pid) for child in children if child.is_running()]
        if remaining:
            subprocess.run(["powershell.exe", "-NoProfile", "-Command",
                            "Stop-Process -Id " + ",".join(remaining) + " -ErrorAction SilentlyContinue"])


CAMPBELL = {
    "black": (12, 12, 12), "red": (197, 15, 31), "green": (19, 161, 14), "brown": (193, 156, 0),
    "blue": (0, 55, 218), "magenta": (136, 23, 152), "cyan": (58, 150, 221), "white": (204, 204, 204),
    "brightblack": (118, 118, 118), "brightred": (231, 72, 86), "brightgreen": (22, 198, 12),
    "brightbrown": (249, 241, 165), "brightblue": (59, 120, 255), "brightmagenta": (180, 0, 158),
    "brightcyan": (97, 214, 214), "brightwhite": (242, 242, 242),
}
BACKGROUND = (12, 12, 12)
FOREGROUND = (204, 204, 204)
CELL_W, CELL_H = 10, 21


def colour(name, bold, dim):
    if name == "default":
        rgb = (242, 242, 242) if bold else FOREGROUND
    elif name in CAMPBELL:
        bright = "bright" + name
        rgb = CAMPBELL[bright] if bold and bright in CAMPBELL else CAMPBELL[name]
    else:
        try:
            rgb = tuple(int(name[i:i + 2], 16) for i in (0, 2, 4))
        except ValueError:
            rgb = FOREGROUND
    if dim:
        rgb = tuple((a + b) // 2 for a, b in zip(rgb, BACKGROUND))
    return rgb


def render(cells, path, title):
    fonts = {
        "latin": ImageFont.truetype(r"C:\Windows\Fonts\consola.ttf", 17),
        "latin_bold": ImageFont.truetype(r"C:\Windows\Fonts\consolab.ttf", 17),
        "cjk": ImageFont.truetype(r"C:\Windows\Fonts\msyh.ttc", 17),
        "symbol": ImageFont.truetype(r"C:\Windows\Fonts\seguisym.ttf", 16),
    }
    rows, columns = len(cells), len(cells[0])
    header = 30
    image = Image.new("RGB", (columns * CELL_W, rows * CELL_H + header), BACKGROUND)
    draw = ImageDraw.Draw(image)
    draw.rectangle([0, 0, columns * CELL_W, header - 2], fill=(32, 32, 40))
    draw.text((8, 5), title, font=fonts["cjk"], fill=(230, 230, 230))
    for y, row in enumerate(cells):
        for x, (data, fg, bold, dim, reverse) in enumerate(row):
            if not data or data == " ":
                continue
            rgb = colour(fg, bold, dim)
            left, top = x * CELL_W, y * CELL_H + header
            if reverse:
                draw.rectangle([left, top, left + CELL_W, top + CELL_H], fill=rgb)
                rgb = BACKGROUND
            code = ord(data[0])
            if 0x2800 <= code <= 0x28FF:
                bits = code - 0x2800
                layout = [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (0, 3), (1, 3)]
                for bit, (dx, dy) in enumerate(layout):
                    if bits & (1 << bit):
                        cx = left + CELL_W * (0.28 + dx * 0.44)
                        cy = top + CELL_H * (0.14 + dy * 0.24)
                        draw.ellipse([cx - 1.6, cy - 1.6, cx + 1.6, cy + 1.6], fill=rgb)
                continue
            if code < 0x2500 or 0x2500 <= code <= 0x257F:
                font = fonts["latin_bold" if bold else "latin"]
            elif code >= 0x2E80:
                font = fonts["cjk"]
            else:
                font = fonts["symbol"]
            draw.text((left, top + 1), data, font=font, fill=rgb)
    image.save(path)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--columns", type=int, default=120)
    parser.add_argument("--lines", type=int, default=40)
    parser.add_argument("--windows-terminal", action="store_true",
                        help="Exercise the Windows Terminal scrollback strategy (WT_SESSION)")
    parser.add_argument("--style", default="classic", help="Magic circle style id, as in /magic list")
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    frames = []
    try:
        with tempfile.TemporaryDirectory(prefix="magicodex-visual-") as directory:
            base = Path(directory)
            home, work = base / "home", base / "work"
            home.mkdir()
            work.mkdir()
            config = f'''
model = "gpt-5.5"
model_provider = "magic_test"
approval_policy = "on-request"
sandbox_mode = "read-only"
web_search = "disabled"
# This runs codex.exe directly: never let its update prompt install another Codex.
check_for_update_on_startup = false
[features]
enable_request_compression = false
multi_agent = false
plugins = false
[model_providers.magic_test]
name = "Local visual fixture"
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
            if args.windows_terminal:
                # Codex picks its Windows Terminal scrollback strategy from WT_SESSION.
                env["WT_SESSION"] = "00000000-0000-4000-8000-000000000000"
            terminal = Terminal(args.binary, env, work, args.columns, args.lines)
            try:
                terminal.wait("Codex")
                time.sleep(1.5)
                terminal.submit(f"/magic {args.style}")
                terminal.wait("Magic circle on")
                time.sleep(1.2)
                frames.append(("01-idle", f"待机：/magic {args.style} 之后", terminal.cells()))
                terminal.submit(PROMPT)
                started = time.monotonic()
                for offset, name, title in [(0.8, "02-submitted", "提交后 0.8s"),
                                            (4.0, "03-charging", "蓄力 4s"),
                                            (8.5, "04-charged", "蓄力 8.5s")]:
                    time.sleep(max(0.0, started + offset - time.monotonic()))
                    frames.append((name, title, terminal.cells()))
                terminal.wait("提交顺序", timeout=30)
                time.sleep(0.4)
                frames.append(("05-commentary", "中间回复环绕", terminal.cells()))
                terminal.wait("法阵已经", timeout=30)
                time.sleep(0.2)
                frames.append(("06-pouring", "最终回复开始流出", terminal.cells()))
                terminal.wait("法阵下方", timeout=30)
                time.sleep(0.2)
                frames.append(("07-pouring-more", "流出中", terminal.cells()))
                terminal.wait("原生快捷键", timeout=30)
                time.sleep(1.5)
                frames.append(("08-complete", "回合完成", terminal.cells()))
            finally:
                terminal.close()
    finally:
        server.shutdown()
        server.server_close()
    for name, title, cells in frames:
        render(cells, args.output / f"{name}.png", f"{args.style} · {title}  ({args.columns}×{args.lines})")
        (args.output / f"{name}.txt").write_text(
            "\n".join("".join(cell[0] or "" for cell in row).rstrip() for row in cells), encoding="utf-8")
    print(json.dumps({"frames": [name for name, _, _ in frames], "events": [
        (kind, round(stamp - Fixture.events[0][1], 2)) for kind, stamp in Fixture.events]}))


if __name__ == "__main__":
    main()
