import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("smoke_naming", Path(__file__).with_name("smoke-naming.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class NamingFixtureTests(unittest.TestCase):
    def test_clean_bash_fixture_clears_hooks_and_cleans_up_on_start_failure(self):
        hooks = {"BASH_ENV": "bad", "ENV": "bad", "PROMPT_COMMAND": "bad",
                 "PS0": "bad", "BASH_FUNC_hook%%": "bad"}
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, hooks), \
                patch.object(module.subprocess, "run") as commands, \
                patch.object(module.smoke_module.Smoke, "start", autospec=True,
                             side_effect=RuntimeError("fixture failure")) as start, \
                patch.object(module.smoke_module.Smoke, "close", autospec=True) as close:
            with self.assertRaisesRegex(RuntimeError, "fixture failure"):
                module.run("/test/zellij", Path("/test/sidebar.wasm"), Path(directory))
            smoke = start.call_args.args[0]
            self.assertTrue(all(key not in smoke.env for key in hooks))
            self.assertEqual(smoke.env["PS1"], "clean> ")
            self.assertIn('args "--noprofile" "--norc"', smoke.layout.read_text())
            self.assertIn('bind "Ctrl p" { MoveFocus "Left"; }',
                          (Path(directory) / "session/config/zellij/config.kdl").read_text())
            close.assert_called_once_with(smoke)
            self.assertEqual(commands.call_count, 3)
            self.assertEqual(commands.call_args_list[-1].args[0][3:5], ["worktree", "add"])


if __name__ == "__main__":
    unittest.main()
