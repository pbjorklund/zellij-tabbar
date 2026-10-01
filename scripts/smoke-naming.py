#!/usr/bin/env python3
"""Check hook-free directory labels in a disposable upstream Zellij session."""

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time

from importlib.util import module_from_spec, spec_from_file_location


spec = spec_from_file_location("sidebar_smoke", Path(__file__).with_name("smoke-zellij.py"))
smoke_module = module_from_spec(spec)
spec.loader.exec_module(smoke_module)


def run(binary, wasm, root):
    repo = root / "repo"
    repo.mkdir()
    outside = root / "outside-dir"
    outside.mkdir()
    worktree = root / "feature-worktree"
    for args in (
        ["init", "-b", "main", str(repo)],
        ["-C", str(repo), "commit", "--allow-empty", "-m", "test: naming fixture"],
        ["-C", str(repo), "worktree", "add", "-b", "feature", str(worktree)],
    ):
        subprocess.run(["git", *args], check=True, capture_output=True, timeout=20)
    fixture = root / "session"
    fixture.mkdir()
    smoke = smoke_module.Smoke(binary, wasm, 20, fixture)
    for key in list(smoke.env):
        if key in ("BASH_ENV", "ENV", "PROMPT_COMMAND", "PS0") or key.startswith("BASH_FUNC_"):
            smoke.env.pop(key)
    smoke.env["PS1"] = "clean> "
    config = fixture / "config/zellij/config.kdl"
    config.write_text(config.read_text().replace('        bind "Ctrl o" { ParkTab; }\n', ""))
    smoke.layout.write_text('''layout {
    tab focus=true {
        pane split_direction="vertical" {
            pane size=38 borderless=true {
                plugin location=''' + json.dumps(smoke.plugin_url) + ''' {
                    format "N:{name}"
                    format_active "N:{name}"
                    border "|"
                    diagnostics "true"
                }
            }
            pane command="/usr/bin/bash" cwd=''' + json.dumps(str(repo)) + ''' {
                args "--noprofile" "--norc"
            }
        }
    }
}
''')

    def expect(name, label):
        deadline = time.monotonic() + smoke.timeout
        while time.monotonic() < deadline:
            smoke.pump()
            row = smoke.screen.lines()[0][:38]
            if smoke_module.sidebar_rows_match([row], ["N:" + name]):
                print(f"PASS {label}: {name}", flush=True)
                return
            text = "\n".join(smoke.screen.lines()).lower()
            if "permission" in "".join(text.split()) and not smoke.granted_panes:
                plugin = next(p for p in smoke.panes() if p.get("plugin_url") == smoke.plugin_url)
                os.write(smoke.master, b"\x10")
                while time.monotonic() < deadline:
                    if any(p["id"] == plugin["id"] and p["is_plugin"] and p["is_focused"]
                           for p in smoke.panes()):
                        os.write(smoke.master, b"y")
                        smoke.granted_panes.add(plugin["id"])
                        break
                    smoke.pump()
        raise RuntimeError(f"{label}: expected {name!r}, got {smoke.screen.lines()!r}")

    def cd(path):
        smoke.cli("action", "write-chars", "cd " + shlex.quote(str(path)) + "\n")

    try:
        smoke.start()
        expect("repo", "clean Bash initial cwd")
        pane = next(p["id"] for p in smoke.panes() if not p["is_plugin"])
        cd(outside)
        expect("outside-dir", "directory change without hooks")
        cd(worktree)
        expect("feature-worktree", "Git worktree directory")
        smoke.cli("action", "write-chars", 'printf "\\033]2;misleading-title\\007"\n')
        # Allow the host to report the title before checking that cwd still wins.
        deadline = time.monotonic() + smoke.timeout
        while time.monotonic() < deadline:
            smoke.pump()
            if any(p["title"] == "misleading-title" for p in smoke.panes() if not p["is_plugin"]):
                break
        else:
            raise RuntimeError("host did not report OSC title")
        expect("feature-worktree", "OSC title cannot replace cwd label")
        smoke.cli("action", "new-pane", "--direction", "right", "--cwd", str(outside),
                  "--", "/usr/bin/bash", "--noprofile", "--norc")
        expect("outside-dir", "focused second pane cwd")
        smoke.cli("action", "move-pane")
        expect("outside-dir", "pane move keeps cwd association")
        smoke.cli("action", "focus-pane-id", f"terminal_{pane}")
        expect("feature-worktree", "focus original pane")
        smoke.cli("action", "rename-tab", "explicit-user-name")
        expect("explicit-user-name", "explicit name")
        cd(repo)
        expect("explicit-user-name", "explicit name survives cd")
        smoke.cli("action", "new-tab", "--name", "other-tab", "--cwd", str(outside))
        smoke.cli("action", "go-to-tab", "1")
        expect("explicit-user-name", "tab switch preserves explicit name")
        smoke.cli("action", "move-tab", "right")
        expect("other-tab", "tab move preserves label order")
    finally:
        smoke.close()
    print("PASS isolated upstream session cleanup", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--zellij", default=shutil.which("zellij"))
    parser.add_argument("--wasm", type=Path,
                        default=Path(__file__).resolve().parents[1] / "target/wasm32-wasip1/release/zellij-tabbar.wasm")
    args = parser.parse_args()
    if not args.zellij or not args.wasm.is_file():
        parser.error("need upstream Zellij and a built WASM")
    with tempfile.TemporaryDirectory(prefix="sidebar-naming-", dir="/tmp") as directory:
        run(args.zellij, args.wasm.resolve(), Path(directory))


if __name__ == "__main__":
    main()
