"""Regression tests for the isolated smoke harness, without launching Zellij."""

import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock


SPEC = importlib.util.spec_from_file_location(
    "smoke_zellij", Path(__file__).with_name("smoke-zellij.py")
)
smoke_zellij = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke_zellij)

# Observed live: the 38-column pane reserves one column for its separator.
# Plugin content starts at row 0, column 0, with its border at column 36.
HEALTHY = [
    "SBtest-A1:alpha                     | adjacent pane",
    "SBtest-I2:beta                      |",
    "SBtest-I3:gamma                     |",
    "                                    |",
]
EXPECTED = ["SBtest-A1:alpha", "SBtest-I2:beta", "SBtest-I3:gamma"]


def broken_sidebars():
    suffix = HEALTHY.copy()
    suffix[0] = "SBtest-A1:alpha-wrong                |"
    missing = HEALTHY.copy()
    missing[1] = "                                     |"
    padding = HEALTHY.copy()
    padding[0] = padding[0][:30] + "!" + padding[0][31:]
    return {
        "wrong suffix": suffix,
        "shifted rows": ["                                     |"] + HEALTHY,
        "shifted columns": [" " + line[:35] + "|" + line[37:] for line in HEALTHY],
        "missing label": missing,
        "reordered rows": [HEALTHY[1], HEALTHY[0], *HEALTHY[2:]],
        "missing border": [line.replace("|", " ") for line in HEALTHY],
        "non-space padding": padding,
        "short viewport": HEALTHY[:2],
    }


class SidebarMatcherTests(unittest.TestCase):
    def test_fixture_has_border_in_column_36(self):
        self.assertTrue(all(line.index("|") == 36 for line in HEALTHY))

    def test_accepts_healthy_sidebar(self):
        self.assertTrue(smoke_zellij.sidebar_matches(HEALTHY, EXPECTED))

    def test_accepts_resize_rounding_without_relaxing_labels(self):
        resized = [
            "SBtest-A1:alpha                      |",
            "SBtest-I2:beta                       |",
            "SBtest-I3:gamma                      |",
        ]
        self.assertTrue(smoke_zellij.sidebar_matches(resized, EXPECTED))

    def test_rejects_malformed_sidebar(self):
        for label, lines in broken_sidebars().items():
            with self.subTest(label=label):
                self.assertFalse(smoke_zellij.sidebar_matches(lines, EXPECTED))

    def test_ignores_adjacent_pane(self):
        self.assertTrue(smoke_zellij.sidebar_matches(HEALTHY, EXPECTED, absent=("adjacent pane",)))

    def test_rejects_stale_name_below_labels(self):
        lines = HEALTHY + ["stale-name                           |"]
        self.assertFalse(smoke_zellij.sidebar_matches(lines, EXPECTED, absent=("stale-name",)))


class ExpectTests(unittest.TestCase):
    def harness(self, lines):
        harness = smoke_zellij.Smoke.__new__(smoke_zellij.Smoke)
        harness.prefix = "SBtest"
        harness.timeout = 1
        harness.screen = mock.Mock()
        harness.screen.lines.return_value = lines
        harness.pump = mock.Mock()
        return harness

    def expect(self, lines, absent=()):
        with mock.patch.object(smoke_zellij.time, "monotonic", side_effect=[0, 0, 2]):
            with contextlib.redirect_stdout(io.StringIO()):
                self.harness(lines).expect(
                    ["alpha", "beta", "gamma"], "alpha", "fixture", absent=absent
                )

    def test_accepts_healthy_rendered_sidebar(self):
        self.expect(HEALTHY)

    def test_rejects_malformed_sidebar(self):
        for label, lines in broken_sidebars().items():
            with self.subTest(label=label):
                with self.assertRaisesRegex(RuntimeError, "timed out waiting for fixture"):
                    self.expect(lines)

    def test_rejects_absent_name_remaining_in_sidebar(self):
        self.expect(HEALTHY, absent=("adjacent pane",))
        with self.assertRaisesRegex(RuntimeError, "timed out"):
            self.expect(HEALTHY + ["stale-name                           |"], absent=("stale-name",))


class CleanupTests(unittest.TestCase):
    def setUp(self):
        self.harness = smoke_zellij.Smoke.__new__(smoke_zellij.Smoke)
        self.harness.session = "isolated-test"
        self.harness.base = ["zellij", "--session", "isolated-test"]
        self.harness.env = {}
        self.harness.root = Path("/unused")
        self.harness.timeout = 1
        self.harness.master = 42
        self.harness.process = mock.Mock(pid=12345)
        self.run = self.enterContext(mock.patch.object(smoke_zellij.subprocess, "run"))
        self.close_fd = self.enterContext(mock.patch.object(smoke_zellij.os, "close"))
        self.killpg = self.enterContext(mock.patch.object(smoke_zellij.os, "killpg"))

    @staticmethod
    def result(code=0, stdout="", stderr=""):
        return subprocess.CompletedProcess(["zellij"], code, stdout, stderr)

    def no_sessions(self):
        # Observed with Zellij 0.45.0 in fully isolated empty directories.
        return self.result(1, stderr="No active zellij sessions found.\n")

    def test_accepts_verified_no_sessions_exit(self):
        self.run.side_effect = [self.result(), self.no_sessions()]
        self.harness.close()
        self.close_fd.assert_called_once_with(42)

    def test_accepts_successful_listing_without_owned_session(self):
        self.run.side_effect = [self.result(), self.result(stdout="other-isolated-session\n")]
        self.harness.close()

    def test_rejects_owned_session_still_listed(self):
        self.run.side_effect = [self.result(), self.result(stdout="isolated-test\n")]
        with self.assertRaisesRegex(RuntimeError, "isolated session did not stop"):
            self.harness.close()
        self.close_fd.assert_called_once_with(42)

    def test_does_not_treat_failed_empty_listing_as_absence(self):
        for result in (
            self.result(2, stderr="socket directory unreadable"),
            self.result(1),
            self.result(-15),
            self.result(2, stderr="No active zellij sessions found.\n"),
            self.result(1, stderr="No active zellij sessions found.\nextra error"),
            self.result(1, stdout="isolated-test\n", stderr="No active zellij sessions found.\n"),
        ):
            with self.subTest(result=result):
                self.harness.master = 42
                self.run.side_effect = [self.result(), result]
                with self.assertRaisesRegex(RuntimeError, "could not verify isolated session cleanup"):
                    self.harness.close()

    def test_kill_failure_is_best_effort_but_listing_is_checked(self):
        for failure in (
            self.result(2, stderr="kill failed"),
            subprocess.TimeoutExpired("kill-session", 1),
            OSError("kill unavailable"),
        ):
            with self.subTest(failure=failure):
                self.harness.master = 42
                self.run.reset_mock()
                self.run.side_effect = [failure, self.no_sessions()]
                self.harness.close()
                self.assertIn("list-sessions", self.run.call_args.args[0])

    def test_listing_timeout_fails_and_closes_pty(self):
        self.run.side_effect = [self.result(), subprocess.TimeoutExpired("list-sessions", 1)]
        with self.assertRaises(subprocess.TimeoutExpired):
            self.harness.close()
        self.close_fd.assert_called_once_with(42)

    def test_wait_timeout_closes_pty(self):
        self.run.return_value = self.result()
        self.harness.process.wait.side_effect = subprocess.TimeoutExpired("wait", 1)
        with self.assertRaises(subprocess.TimeoutExpired):
            self.harness.close()
        self.close_fd.assert_called_once_with(42)

    def test_process_signal_failure_closes_pty(self):
        self.run.return_value = self.result()
        self.harness.process.wait.side_effect = subprocess.TimeoutExpired("wait", 1)
        self.killpg.side_effect = OSError("signal failed")
        with self.assertRaises(OSError):
            self.harness.close()
        self.close_fd.assert_called_once_with(42)

    def test_start_failure_without_process_closes_pty(self):
        self.harness.process = None
        self.harness.close()
        self.close_fd.assert_called_once_with(42)
        self.run.assert_not_called()


class MainTests(unittest.TestCase):
    def run_main(self, primary=None, cleanup=None):
        output = io.StringIO()
        harness = mock.Mock()
        harness.run.side_effect = primary
        harness.close.side_effect = cleanup
        harness.screen.lines.return_value = ["last frame"]
        with tempfile.TemporaryDirectory() as directory:
            wasm = Path(directory) / "test.wasm"
            wasm.write_bytes(b"fixture")
            args = mock.Mock(wasm=wasm, timeout=1)
            with mock.patch.object(smoke_zellij.argparse.ArgumentParser, "parse_args", return_value=args), \
                 mock.patch.object(smoke_zellij.shutil, "which", return_value="/unused/zellij"), \
                 mock.patch.object(smoke_zellij, "Smoke", return_value=harness), \
                 contextlib.redirect_stdout(output), contextlib.redirect_stderr(output):
                status = smoke_zellij.main()
        harness.close.assert_called_once_with()
        return status, output.getvalue()

    def test_reports_primary_and_cleanup_failure_without_replacing_primary(self):
        status, output = self.run_main(RuntimeError("primary failed"), RuntimeError("cleanup failed"))
        self.assertEqual(status, 1)
        self.assertIn("FAIL: primary failed", output)
        self.assertIn("FAIL cleanup: cleanup failed", output)
        self.assertLess(output.index("primary failed"), output.index("cleanup failed"))

    def test_cleanup_timeout_after_success_is_reported_as_failure(self):
        status, output = self.run_main(cleanup=subprocess.TimeoutExpired("list-sessions", 1))
        self.assertEqual(status, 1)
        self.assertIn("FAIL cleanup:", output)
        self.assertNotIn("PASS live PTY", output)

    def test_primary_timeout_is_reported_and_cleanup_runs(self):
        status, output = self.run_main(primary=subprocess.TimeoutExpired("run", 1))
        self.assertEqual(status, 1)
        self.assertIn("FAIL:", output)

    def test_success_still_runs_cleanup_and_reports_success(self):
        status, output = self.run_main()
        self.assertEqual(status, 0)
        self.assertIn("PASS live PTY", output)


if __name__ == "__main__":
    unittest.main()
