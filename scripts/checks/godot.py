"""Run every Godot smoke check with bounded execution and useful failure output."""

import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


ROOT = Path(__file__).resolve().parents[2]
ERROR = re.compile(r"SCRIPT ERROR|SHADER ERROR|^ERROR:", re.MULTILINE)
HEADLESS = """
ride ui group_ui seasons touch summary workouts devices window dialog settings benchmark
tree_lod hud course course_loading rider profile analysis upload error moment ride_clock
""".split()


def run_check(name, command, marker=None, seconds=180):
    """Fail on crashes, Godot errors, missing completion or a hung process tree."""
    print(f"Starting {name} (limit {seconds}s)", flush=True)
    started = time.monotonic()
    with subprocess.Popen(
        command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, start_new_session=True
    ) as process:
        try:
            output, _ = process.communicate(timeout=seconds)
        except subprocess.TimeoutExpired:
            # Xvfb and Godot must both stop; killing only xvfb-run leaves Godot behind.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass  # The process group exited just as its deadline expired.
            output, _ = process.communicate()
            print(output.decode(errors="replace"), flush=True)
            raise RuntimeError(f"{name} timed out after {seconds}s") from None
    text = output.decode(errors="replace")
    completed = marker is None or re.search(marker, text, re.MULTILINE)
    if process.returncode or ERROR.search(text) or not completed:
        print(text, flush=True)
        raise RuntimeError(f"{name} failed (exit {process.returncode}, expected {marker!r})")
    print(f"Passed {name} ({time.monotonic() - started:.1f}s)", flush=True)


def main():
    """Import once, then run the main scene, headless suites and display checks in order."""
    godot = ["godot", "--path", str(ROOT / "app")]
    run_check("Godot import", [*godot, "--headless", "--import"])
    run_check("Main scene", [*godot, "--headless", "--quit-after", "10"], r"^ORQWA ")
    for name in HEADLESS:
        run_check(
            name,
            [*godot, "--headless", "-s", f"res://tests/{name}_smoke.gd"],
            f"{name.upper().replace('_', ' ')} SMOKE TEST PASSED",
        )
    for name, seconds, options in (
        ("surface", 120, ["--rendering-driver", "vulkan", "--resolution", "800x240"]),
        ("nameplate_render", 90, ["--rendering-driver", "vulkan", "--resolution", "800x450"]),
        ("idle_pointer", 90, []),
    ):
        run_check(
            name,
            [
                "xvfb-run", "-a", *godot, "--audio-driver", "Dummy", *options,
                "-s", f"res://tests/{name}_smoke.gd",
            ],
            f"{name.upper().replace('_', ' ')} SMOKE TEST PASSED",
            seconds,
        )


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as error:
        print(error, file=sys.stderr)
        sys.exit(1)
