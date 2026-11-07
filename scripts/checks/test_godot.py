"""Failures in a smoke script must fail CI with the original diagnostic."""

import contextlib
import io
import sys
import unittest

from godot import run_check


class SmokeRunnerTests(unittest.TestCase):
    def setUp(self):
        self.output = io.StringIO()
        redirect = contextlib.redirect_stdout(self.output)
        redirect.__enter__()
        self.addCleanup(redirect.__exit__, None, None, None)

    def run_script(self, script, **options):
        return run_check("fixture", [sys.executable, "-u", "-c", script], "PASSED", **options)

    def test_completion_marker_is_required(self):
        self.run_script('print("PASSED")')
        with self.assertRaisesRegex(RuntimeError, "failed"):
            self.run_script('print("stopped before assertions")')

    def test_script_and_shader_errors_fail_even_after_a_success_marker(self):
        for error in ("SCRIPT ERROR", "SHADER ERROR", "ERROR:"):
            with self.subTest(error=error), self.assertRaisesRegex(RuntimeError, "failed"):
                self.run_script(f'print("PASSED\\n{error} original diagnostic")')

    def test_nonzero_exit_preserves_output(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), self.assertRaisesRegex(RuntimeError, "exit 7"):
            self.run_script('print("original diagnostic"); raise SystemExit(7)')
        self.assertIn("original diagnostic", output.getvalue())

    def test_hung_script_preserves_output_and_fails(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), self.assertRaisesRegex(RuntimeError, "timed out"):
            self.run_script('import time; print("before hang"); time.sleep(60)', seconds=0.2)
        self.assertIn("before hang", output.getvalue())

    def test_timeout_also_stops_children_holding_the_output_pipe(self):
        # Like xvfb-run, the parent leaves a long-running child on the same output pipe.
        with self.assertRaisesRegex(RuntimeError, "timed out"):
            self.run_script(
                'import subprocess, sys, time; '
                'subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"]); '
                'time.sleep(60)',
                seconds=0.2,
            )


if __name__ == "__main__":
    unittest.main()
