#!/usr/bin/env python3
"""Run an isolated live sidebar smoke test. Requires Python 3 and Zellij.

Usage: python3 scripts/smoke-zellij.py [--zellij PATH] [--wasm PATH] [--timeout 20]
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
import shlex
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


def sidebar_rows_match(rows, expected):
    """Require complete ASCII labels at column 0, space padding and a border."""
    if len(rows) != len(expected):
        return False
    for row, label in zip(rows, expected):
        text, border, remainder = row.rpartition("|")
        if text.rstrip(" ") != label or not border or remainder.strip(" "):
            return False
    return True


def sidebar_matches(lines, expected, absent=()):
    """Match labels anchored to the top of the sidebar."""
    sidebar = [line[:SIDEBAR_WIDTH] for line in lines]
    return (sidebar_rows_match(sidebar[:len(expected)], expected)
            and not any(name in "\n".join(sidebar) for name in absent))


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
    normal {
        bind "Ctrl p" { MoveFocus "Left"; }
    }
}
''', encoding="utf-8")
        self.base = [binary, "--session", self.session, "--config", str(config), "--config-dir", str(config_dir), "--data-dir", str(root / "data")]
        # Markers occur only in plugin output, not terminal commands or tab names.
        self.prefix = "SB" + uuid.uuid4().hex[:4]
        self.layout = root / "layout.kdl"
        self.plugin_url = "file:" + str(wasm)
        plugin_url = json.dumps(self.plugin_url)
        self.heartbeat = root / "alpha-heartbeat"
        heartbeat = shlex.quote(str(self.heartbeat))
        alpha_command = json.dumps(
            f'i=0; while :; do i=$((i + 1)); printf "alive-%s\\n" "$i"; '
            f'printf "%s\\n" "$i" >> {heartbeat}; sleep 0.1; done'
        )
        gamma_cwd = root / "backoffice"
        gamma_cwd.mkdir()
        gamma_cwd_kdl = json.dumps(str(gamma_cwd))
        gamma_command = json.dumps(r"printf '\033]0;π - planning - backoffice\007'; exec sleep 600", ensure_ascii=False)
        self.new_tab_layout = f'''layout {{
    tab {{
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
            pane command="/bin/sh" {{ args "-c" "exec sleep 600"; }}
        }}
    }}
}}'''
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
    tab name="alpha" focus=true {{ pane command="/bin/sh" {{ args "-c" {alpha_command}; }}; }}
    tab name="beta" {{ pane command="/bin/sh" {{ args "-c" "exec sleep 600"; }}; }}
    tab name="gamma" {{ pane command="/bin/sh" cwd={gamma_cwd_kdl} {{ args "-c" {gamma_command}; }}; }}
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

    def expect_labels(self, expected, active, label, absent=()):
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

    def expect(self, names, active, label, absent=(), watchers=None):
        watchers = watchers or {}
        expected = [f"{self.prefix}-{'A' if name == active else 'I'}{i}:"
                    f"{watchers[name] + ' ' if watchers.get(name) else ''}{name}"
                    for i, name in enumerate(names, 1)]
        self.expect_labels(expected, active, label, absent=absent)

    def expect_animation(self, names, active, target, watchers=""):
        frames = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"
        seen = None
        deadline = time.monotonic() + self.timeout
        while time.monotonic() < deadline:
            self.pump()
            lines = self.screen.lines()
            for frame in frames:
                marked = [f"{watchers}{frame} {name}" if name == target else name for name in names]
                expected = [f"{self.prefix}-{'A' if names[i - 1] == active else 'I'}{i}:{name}"
                            for i, name in enumerate(marked, 1)]
                if sidebar_matches(lines, expected):
                    if seen is not None and seen != frame:
                        print(f"PASS local status animation: {seen} -> {frame}", flush=True)
                        return
                    seen = frame
                    break
        raise RuntimeError(f"timed out waiting for local status animation; first={seen!r}")

    def pane_for_tab(self, tab_name):
        return next(pane["id"] for pane in self.panes()
                    if pane.get("tab_name") == tab_name and not pane.get("is_plugin"))

    def status(self, pane_id, mode, seq, watchers=None, **detail):
        payload = {
            "v": 1, "kind": "remove" if mode == "remove" else "snapshot",
            "runtime_id": "smoke", "seq": seq, "pane_id": pane_id,
        }
        if mode != "remove":
            payload.update(mode=mode, **detail)
            if watchers is not None:
                payload["watchers"] = watchers
        self.cli("pipe", "--name", "pi_status", "--", json.dumps(payload))

    def resize(self, rows, cols):
        self.screen.resize(rows, cols)
        fcntl.ioctl(self.master, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))
        os.kill(self.process.pid, signal.SIGWINCH)

    def click_sidebar(self, row):
        y = row + 1
        os.write(self.master, f"\x1b[<0;3;{y}M\x1b[<0;3;{y}m".encode())

    def wheel_sidebar(self, forward):
        button = 65 if forward else 64
        os.write(self.master, f"\x1b[<{button};3;1M".encode())

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
        # Visit each initial plugin pane so first-run permission prompts cannot
        # hold a broadcast pipe open through Zellij's backpressure mechanism.
        for index, name in [(2, "beta"), (3, "gamma"), (1, "alpha")]:
            self.cli("action", "go-to-tab", str(index))
            self.expect(["alpha", "beta", "gamma"], name, f"approve {name} instance")

        self.expect_labels(
            [f"{self.prefix}-A1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-I3:gamma"]
            + [""] * 21, "alpha", "all remaining sidebar rows are empty",
        )
        self.wheel_sidebar(True)
        self.expect(["alpha", "beta", "gamma"], "beta", "wheel forward")
        self.wheel_sidebar(False)
        self.expect(["alpha", "beta", "gamma"], "alpha", "wheel back")

        gamma_pane = self.pane_for_tab("gamma")
        detail = {"C": {"status": "error"}, "P": {"status": "working"}}
        self.status(gamma_pane, "base", 1, folder="backoffice", watchers="CP", watcher_states=detail)
        self.expect(["alpha", "beta", "gamma Ce|Pw"], "alpha", "detailed watcher suffix")
        self.cli("action", "go-to-tab", "3")
        self.cli("action", "rename-tab", "Tab #3")
        self.expect_labels([f"{self.prefix}-I1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-A3:backoffice Ce|Pw"], "Tab #3", "automatic folder label")
        detail["P"] = {"status": "polling"}
        self.status(gamma_pane, "base", 2, folder="ampliflow-iac", watchers="CP", watcher_states=detail)
        self.expect_labels([f"{self.prefix}-I1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-A3:backoffice Ce|Pp"], "Tab #3", "state-only update keeps native folder ahead of bridge fallback")
        self.cli("action", "rename-tab", "gamma")
        self.status(gamma_pane, "base", 3, watchers="P", watcher_states={"P": {"status": "working"}})
        self.expect_labels([f"{self.prefix}-I1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-A3:gamma Pw"], "gamma", "project-only working")
        self.cli("action", "go-to-tab", "1")
        self.expect(["alpha", "beta", "gamma Pw"], "alpha", "leave watcher tab before completion")
        self.status(gamma_pane, "done", 4, watchers="P", watcher_states={"P": {"status": "working"}})
        self.expect(["alpha", "beta", "● gamma Pw"], "alpha", "completion independent of watcher")
        self.cli("action", "go-to-tab", "3")
        self.expect_labels([f"{self.prefix}-I1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-A3:gamma Pw"], "gamma", "view clears completion only")
        self.status(gamma_pane, "base", 5, watchers="CIPRS", watcher_states={letter: {"status": "polling"} for letter in "SRIPC"})
        self.expect_labels([f"{self.prefix}-I1:alpha", f"{self.prefix}-I2:beta", f"{self.prefix}-A3:gamma Cp|Pp|Ip|Rp|Sp"], "gamma", "all-five canonical order")
        self.status(gamma_pane, "base", 6)
        self.cli("action", "go-to-tab", "1")
        self.expect(["alpha", "beta", "gamma"], "alpha", "off clears watcher suffix")

        alpha_pane = self.pane_for_tab("alpha")
        names = ["alpha", "beta", "gamma"]
        self.status(alpha_pane, "base", 1, watchers="SRPICC")
        self.expect(names, "alpha", "idle watcher prefix", watchers={"alpha": "CIPRS"})
        self.status(alpha_pane, "working", 2, watchers="CIPRS")
        self.expect_animation(names, "alpha", "alpha", watchers="CIPRS")
        self.status(alpha_pane, "base", 3, watchers="IPRS")
        self.expect(names, "alpha", "clear work and one watcher but keep others", watchers={"alpha": "IPRS"})
        self.status(alpha_pane, "base", 4, watchers="")
        self.expect(names, "alpha", "watchers off")
        self.status(alpha_pane, "base", 5, watchers="I")
        self.expect(names, "alpha", "watcher before removal", watchers={"alpha": "I"})
        self.status(alpha_pane, "remove", 6)
        self.expect(names, "alpha", "removal clears watchers")
        self.click_sidebar(1)
        self.expect(names, "beta", "click away from watcher tab")
        self.status(alpha_pane, "done", 7, watchers="CIPRS")
        self.expect(["CIPRS● alpha", "beta", "gamma"], "beta", "watchers before background completion")
        self.click_sidebar(0)
        self.expect(names, "alpha", "view clears completion but keeps watchers", watchers={"alpha": "CIPRS"})
        self.status(alpha_pane, "base", 8)
        self.expect(names, "alpha", "omitted watchers clear prefix")
        for index, letter in enumerate("CIPRS"):
            seq = 9 + index * 3
            self.status(alpha_pane, "base", seq, watchers=letter)
            self.expect(names, "alpha", f"idle watcher {letter}", watchers={"alpha": letter})
            self.status(alpha_pane, "working", seq + 1, watchers=letter)
            self.expect_animation(names, "alpha", "alpha", watchers=letter)
            self.status(alpha_pane, "base", seq + 2, watchers="")
            self.expect(names, "alpha", f"watcher {letter} off")
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
        self.click_sidebar(1)
        self.expect(["alpha", "small"], "small", "click second sidebar row")
        self.click_sidebar(0)
        self.expect(["alpha", "small"], "alpha", "click first sidebar row")
        time.sleep(0.2)

        added_names = [f"overflow-{i}" for i in range(3, 10)]
        all_names = ["alpha", "small"]
        for name in added_names:
            created_tab_id = self.cli(
                "action", "new-tab", "--name", name,
                "--layout-string", self.new_tab_layout,
            ).strip()
            if not created_tab_id.isdigit():
                raise RuntimeError(f"new-tab returned invalid tab ID for {name}: {created_tab_id!r}")
            all_names.append(name)
            if not any(pane.get("tab_name") == name for pane in self.panes()):
                raise RuntimeError(
                    f"new-tab returned {created_tab_id} but {name} is absent from list-panes"
                )
            self.cli("action", "go-to-tab-name", name)
            self.expect(all_names, name, f"approve overflow tab {name}")

        self.cli("action", "go-to-tab-name", "alpha")
        self.resize(6, 70)
        self.expect_labels(
            [f"{self.prefix}-A1:alpha", f"{self.prefix}-I2:small"]
            + [f"{self.prefix}-I{i}:overflow-{i}" for i in range(3, 5)]
            + ["  v +5", ""], "alpha", "constrained overflow below",
            absent=("overflow-5", "overflow-6", "overflow-7", "overflow-8", "overflow-9"),
        )
        self.click_sidebar(4)
        self.expect_labels(
            ["  ^ +2"] + [f"{self.prefix}-{'A' if i == 5 else 'I'}{i}:overflow-{i}"
                           for i in range(3, 7)] + ["  v +3"],
            "overflow-5", "overflow click reveals next hidden tab",
        )

        log_text = "\n".join(path.read_text(errors="replace") for path in self.root.rglob("*.log"))
        if "event=loaded " not in log_text or "event=render " not in log_text:
            raise RuntimeError("plugin diagnostics missing from isolated Zellij logs")
        if "zellij-tabbar version=" in "\n".join(self.screen.lines()):
            raise RuntimeError("diagnostics leaked into the terminal viewport")
        print("PASS diagnostics reach Zellij logs, not the sidebar", flush=True)
        plugins = [pane for pane in self.panes() if pane.get("plugin_url") == self.plugin_url]
        if len(plugins) != len(all_names) or any(
                pane["exited"] or pane["is_selectable"] for pane in plugins):
            raise RuntimeError(
                f"expected {len(all_names)} live, non-selectable sidebar instances"
            )


def resolve_zellij(value):
    candidate = value.expanduser()
    if not candidate.is_absolute() and candidate.parent == Path("."):
        found = shutil.which(str(candidate))
        if found is not None:
            candidate = Path(found)
    return candidate.resolve()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--zellij", type=Path,
        default=Path("zellij"),
        help="upstream Zellij executable (default: zellij on PATH)",
    )
    parser.add_argument("--wasm", type=Path, default=Path(__file__).resolve().parents[1] / "target/wasm32-wasip1/release/zellij-tabbar.wasm")
    parser.add_argument("--timeout", type=float, default=20, help="seconds per bounded action/assertion")
    args = parser.parse_args()
    binary = resolve_zellij(args.zellij)
    if (not binary.is_file() or not os.access(binary, os.X_OK)
            or not args.wasm.is_file() or not math.isfinite(args.timeout)
            or args.timeout <= 0):
        parser.error("need an executable Zellij, an existing WASM, and a positive timeout")
    wasm = args.wasm.resolve()
    print(
        f"ZELLIJ {binary}\nWASM {wasm}\n"
        f"SHA256 {hashlib.sha256(wasm.read_bytes()).hexdigest()}",
        flush=True,
    )
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
    print("PASS live PTY rendering, constrained overflow, mouse clicks and wheel navigation; Unicode and activity rows covered by host tests")
    return 0


if __name__ == "__main__":
    sys.exit(main())
