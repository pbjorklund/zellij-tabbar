#!/usr/bin/env python3
"""Run an isolated live sidebar smoke test. Requires Python 3 and Zellij.

Usage: python3 scripts/smoke-zellij.py [--wasm PATH] [--timeout 20]
No user config, installed plugin, or existing session is changed.
"""

import argparse
import codecs
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import pty
import select
import shutil
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unicodedata
import uuid


SIDEBAR_WIDTH = 38


def sidebar_matches(lines, expected, absent=()):
    """Require complete ASCII labels at row/col 0, space padding and a border."""
    sidebar = [line[:SIDEBAR_WIDTH] for line in lines]
    if len(sidebar) < len(expected):
        return False
    for row, label in zip(sidebar, expected):
        text, border, remainder = row.partition("|")
        if text.rstrip(" ") != label or not border or remainder.strip(" "):
            return False
    return not any(name in "\n".join(sidebar) for name in absent)


class Screen:
    """Small VT viewport oracle, not a general terminal emulator."""

    def __init__(self, rows, cols):
        self.resize(rows, cols)
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.pending = ""
        self.saved = (0, 0)

    def resize(self, rows, cols):
        self.rows, self.cols = rows, cols
        self.cells = [[" "] * cols for _ in range(rows)]
        self.row = self.col = 0

    def feed(self, data):
        text = self.pending + self.decoder.decode(data)
        self.pending = ""
        i = 0
        while i < len(text):
            char = text[i]
            if char == "\x1b":
                start = i
                if i + 1 == len(text):
                    self.pending = text[start:]
                    break
                kind = text[i + 1]
                if kind == "[":
                    i += 2
                    parameters = i
                    while i < len(text) and not ("@" <= text[i] <= "~"):
                        i += 1
                    if i == len(text):
                        self.pending = text[start:]
                        break
                    self.csi(text[parameters:i], text[i])
                elif kind in "]P_^":
                    i += 2
                    while i < len(text) and text[i] != "\x07" and text[i:i + 2] != "\x1b\\":
                        i += 1
                    if i == len(text) or text[i:] == "\x1b":
                        self.pending = text[start:]
                        break
                    if text[i] == "\x1b":
                        i += 1
                elif kind in "()":
                    if i + 2 >= len(text):
                        self.pending = text[start:]
                        break
                    i += 2
                else:
                    if kind == "7":
                        self.saved = (self.row, self.col)
                    elif kind == "8":
                        self.row, self.col = self.saved
                    i += 1
            elif char == "\r":
                self.col = 0
            elif char == "\n":
                self.row = min(self.rows - 1, self.row + 1)
            elif char == "\b":
                self.col = max(0, self.col - 1)
            elif char == "\t":
                self.col = min(self.cols - 1, (self.col // 8 + 1) * 8)
            elif char >= " " and char != "\x7f":
                width = 0 if unicodedata.combining(char) else (2 if unicodedata.east_asian_width(char) in "WF" else 1)
                if width and self.col < self.cols:
                    self.cells[self.row][self.col] = char
                    if width == 2 and self.col + 1 < self.cols:
                        self.cells[self.row][self.col + 1] = ""
                    self.col += width
            i += 1
        # A malformed escape sequence must not grow the buffer without bound.
        if len(self.pending) > 16384:
            raise RuntimeError("oversized terminal escape sequence")

    def csi(self, parameters, command):
        if parameters.startswith(("?", ">", "=")):
            if parameters == "?1049" and command == "h":
                self.resize(self.rows, self.cols)
            return
        try:
            values = [int(value or 0) for value in parameters.split(";")]
        except ValueError:
            return
        first = values[0]
        count = first or 1
        if command in "Hf":
            self.row = min(self.rows - 1, max(0, count - 1))
            self.col = min(self.cols - 1, max(0, (values[1] or 1) - 1)) if len(values) > 1 else 0
        elif command == "A":
            self.row = max(0, self.row - count)
        elif command == "B":
            self.row = min(self.rows - 1, self.row + count)
        elif command == "C":
            self.col = min(self.cols - 1, self.col + count)
        elif command == "D":
            self.col = max(0, self.col - count)
        elif command == "G":
            self.col = min(self.cols - 1, max(0, count - 1))
        elif command == "d":
            self.row = min(self.rows - 1, max(0, count - 1))
        elif command == "J":
            for row in range(self.rows):
                for col in range(self.cols):
                    if first in (2, 3) or (first == 0 and (row, col) >= (self.row, self.col)) or (first == 1 and (row, col) <= (self.row, self.col)):
                        self.cells[row][col] = " "
        elif command == "K":
            for col in range(self.cols):
                if first == 2 or (first == 0 and col >= self.col) or (first == 1 and col <= self.col):
                    self.cells[self.row][col] = " "
        elif command == "X":
            for col in range(self.col, min(self.cols, self.col + count)):
                self.cells[self.row][col] = " "
        elif command == "s":
            self.saved = (self.row, self.col)
        elif command == "u":
            self.row, self.col = self.saved

    def lines(self):
        return ["".join(row) for row in self.cells]


class Smoke:
    def __init__(self, binary, wasm, timeout, root):
        self.session = "tabbar-smoke-" + uuid.uuid4().hex[:16]
        self.timeout = timeout
        self.root = root
        self.env = {key: value for key, value in os.environ.items() if not key.startswith("ZELLIJ")}
        for key, directory in {
            "HOME": "home", "XDG_CONFIG_HOME": "config", "XDG_CACHE_HOME": "cache",
            "XDG_DATA_HOME": "data", "XDG_STATE_HOME": "state", "XDG_RUNTIME_DIR": "runtime",
            "TMPDIR": "tmp", "ZELLIJ_SOCKET_DIR": "sockets",
        }.items():
            path = root / directory
            path.mkdir(mode=0o700)
            self.env[key] = str(path)
        self.env.update(TERM="xterm-256color", SHELL="/bin/sh")
        config_dir = root / "config" / "zellij"
        config_dir.mkdir()
        config = config_dir / "config.kdl"
        config.write_text('''default_shell "/bin/sh"
pane_frames false
session_serialization false
show_startup_tips false
show_release_notes false
on_force_close "quit"
keybinds clear-defaults=true {
    normal { bind "Ctrl p" { MoveFocus "Left"; }; }
}
''', encoding="utf-8")
        self.base = [binary, "--session", self.session, "--config", str(config), "--config-dir", str(config_dir), "--data-dir", str(root / "data")]
        # Markers occur only in plugin output, not terminal commands or tab names.
        self.prefix = "SB" + uuid.uuid4().hex[:4]
        self.layout = root / "layout.kdl"
        self.plugin_url = "file:" + str(wasm)
        plugin_url = json.dumps(self.plugin_url)
        self.layout.write_text(f'''layout {{
    default_tab_template {{
        pane split_direction="vertical" {{
            pane size={SIDEBAR_WIDTH} borderless=true {{
                plugin location={plugin_url} {{
                    format "{self.prefix}-I{{index}}:{{name}}"
                    format_active "{self.prefix}-A{{index}}:{{name}}"
                    max_name_length 22
                    diagnostics "true"
                    border "|"
                }}
            }}
            children
        }}
    }}
    tab name="alpha" focus=true {{ pane command="/bin/sh" {{ args "-c" "exec sleep 600"; }}; }}
    tab name="beta" {{ pane command="/bin/sh" {{ args "-c" "exec sleep 600"; }}; }}
    tab name="gamma" {{ pane command="/bin/sh" {{ args "-c" "exec sleep 600"; }}; }}
}}
''', encoding="utf-8")
        self.screen = Screen(24, 100)
        self.master = None
        self.process = None
        self.granted_panes = set()

    def _cli_result(self, *action):
        return subprocess.run(self.base + list(action), env=self.env, cwd=self.root,
                              stdin=subprocess.DEVNULL, capture_output=True, text=True,
                              timeout=self.timeout)

    def cli(self, *action, check=True):
        result = self._cli_result(*action)
        if check and result.returncode:
            raise RuntimeError(f"{action}: {result.stderr or result.stdout}")
        return result.stdout

    def panes(self):
        output = self.cli("action", "list-panes", "--all", "--json")
        if not output.strip():
            return []
        return json.loads(output)

    def start(self):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 100, 0, 0))

        def terminal_session():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        try:
            self.process = subprocess.Popen(self.base + ["--new-session-with-layout", str(self.layout)],
                                            env=self.env, cwd=self.root, stdin=slave, stdout=slave,
                                            stderr=slave, preexec_fn=terminal_session)
        finally:
            os.close(slave)

    def pump(self, timeout=0.1):
        if select.select([self.master], [], [], timeout)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError as error:
                raise RuntimeError(f"Zellij PTY closed: {error}") from error
            if not data:
                raise RuntimeError("Zellij PTY reached EOF")
            self.screen.feed(data)
        if self.process.poll() is not None:
            raise RuntimeError(f"Zellij exited: {self.process.returncode}")

    def expect(self, names, active, label, absent=()):
        expected = [f"{self.prefix}-{'A' if name == active else 'I'}{i}:{name}"
                    for i, name in enumerate(names, 1)]
        deadline = time.monotonic() + self.timeout
        while time.monotonic() < deadline:
            self.pump()
            lines = self.screen.lines()
            if sidebar_matches(lines, expected, absent):
                print(f"PASS {label}: " + ", ".join(expected), flush=True)
                return
            text = "\n".join(lines).lower()
            if "permission" in "".join(text.split()) and ("[y]" in text or "(y)" in text or "(y/n)" in text):
                candidates = [pane for pane in self.panes() if pane.get("plugin_url") == self.plugin_url and pane.get("tab_name") == active]
                if not candidates:
                    continue
                pane_id = candidates[0]["id"]
                if pane_id not in self.granted_panes:
                    os.write(self.master, b"\x10")
                    while time.monotonic() < deadline:
                        if any(pane["id"] == pane_id and pane["is_plugin"] and pane["is_focused"] for pane in self.panes()):
                            os.write(self.master, b"y")
                            self.granted_panes.add(pane_id)
                            break
                        self.pump()
        raise RuntimeError(f"timed out waiting for {label}; expected {expected}\n" + "\n".join(self.screen.lines()))

    def resize(self, rows, cols):
        self.screen.resize(rows, cols)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)

    def close(self):
        try:
            if self.process is not None:
                try:
                    self.cli("kill-session", self.session, check=False)
                except (subprocess.TimeoutExpired, OSError):
                    pass
                try:
                    self.process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(self.process.pid, signal.SIGTERM)
                    try:
                        self.process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        os.killpg(self.process.pid, signal.SIGKILL)
                        self.process.wait(timeout=3)
        finally:
            if self.master is not None:
                os.close(self.master)
                self.master = None
        if self.process is not None:
            result = self._cli_result("list-sessions", "--short", "--no-formatting")
            # Zellij 0.45.0 reports an empty socket directory with this exact
            # nonzero result, verified in isolated HOME/XDG/socket directories.
            no_sessions = (result.returncode == 1 and result.stdout == ""
                           and result.stderr.strip() == "No active zellij sessions found.")
            if result.returncode != 0 and not no_sessions:
                raise RuntimeError(
                    f"could not verify isolated session cleanup: exit {result.returncode}; "
                    f"stdout={result.stdout!r}; stderr={result.stderr!r}"
                )
            if self.session in result.stdout.splitlines():
                raise RuntimeError(f"isolated session did not stop: {self.session}")

    def run(self):
        self.start()
        self.expect(["alpha", "beta", "gamma"], "alpha", "initial custom labels")
        self.cli("action", "go-to-tab", "2")
        self.expect(["alpha", "beta", "gamma"], "beta", "switch tab")
        self.cli("action", "rename-tab", "renamed")
        self.expect(["alpha", "renamed", "gamma"], "renamed", "rename tab", absent=("beta",))
        self.cli("action", "move-tab", "right")
        self.expect(["alpha", "gamma", "renamed"], "renamed", "move tab")
        self.cli("action", "close-tab")
        self.expect(["alpha", "gamma"], "alpha", "close tab", absent=("renamed",))
        self.cli("action", "go-to-tab", "2")
        self.expect(["alpha", "gamma"], "gamma", "switch to third instance")
        self.resize(12, 70)
        self.expect(["alpha", "gamma"], "gamma", "resize smaller")
        self.cli("action", "rename-tab", "small")
        self.expect(["alpha", "small"], "small", "update after resize", absent=("gamma",))
        self.resize(30, 110)
        self.expect(["alpha", "small"], "small", "resize larger")
        self.cli("action", "go-to-tab", "1")
        self.expect(["alpha", "small"], "alpha", "switch back to original instance")
        os.write(self.master, b"\x1b[<0;3;2M\x1b[<0;3;2m")
        self.expect(["alpha", "small"], "small", "click second sidebar row")
        os.write(self.master, b"\x1b[<0;3;1M\x1b[<0;3;1m")
        self.expect(["alpha", "small"], "alpha", "click first sidebar row")
        log_text = "\n".join(path.read_text(errors="replace") for path in self.root.rglob("*.log"))
        if "event=loaded " not in log_text or "event=render " not in log_text:
            raise RuntimeError("plugin diagnostics missing from isolated Zellij logs")
        if "zellij-tabbar version=" in "\n".join(self.screen.lines()):
            raise RuntimeError("diagnostics leaked into the terminal viewport")
        print("PASS diagnostics reach Zellij logs, not the sidebar", flush=True)
        plugins = [pane for pane in self.panes() if pane.get("plugin_url") == self.plugin_url]
        if len(plugins) != 2 or any(pane["exited"] or pane["is_selectable"] for pane in plugins):
            raise RuntimeError("expected two live, non-selectable sidebar instances")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wasm", type=Path, default=Path(__file__).resolve().parents[1] / "target/wasm32-wasip1/release/zellij-tabbar.wasm")
    parser.add_argument("--timeout", type=float, default=20, help="seconds per bounded action/assertion")
    args = parser.parse_args()
    binary = shutil.which("zellij")
    if binary is None or not args.wasm.is_file() or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error("need zellij in PATH, an existing WASM, and a positive timeout")
    wasm = args.wasm.resolve()
    print(f"WASM {wasm}\nSHA256 {hashlib.sha256(wasm.read_bytes()).hexdigest()}", flush=True)
    with tempfile.TemporaryDirectory(prefix="tabbar-smoke-", dir="/tmp") as directory:
        smoke = Smoke(binary, wasm, args.timeout, Path(directory))
        print(f"Isolated session: {smoke.session}", flush=True)
        failed = False
        try:
            smoke.run()
        except (RuntimeError, subprocess.TimeoutExpired, OSError, ValueError) as error:
            print(f"FAIL: {error}\nLast viewport:\n" + "\n".join(smoke.screen.lines()), file=sys.stderr)
            failed = True
        finally:
            try:
                smoke.close()
            except (RuntimeError, subprocess.TimeoutExpired, OSError, ValueError) as error:
                print(f"FAIL cleanup: {error}", file=sys.stderr)
                failed = True
        if failed:
            return 1
    print("PASS live PTY rendering and mouse clicks; overflow, Unicode and activity rows covered only by host tests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
