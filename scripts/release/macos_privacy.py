#!/usr/bin/env python3
"""Reject a macOS artifact that cannot request microphone access as ORQWA."""

import pathlib
import plistlib
import subprocess
import sys


def problems(info: dict, entitlements: dict) -> list[str]:
    found = []
    if info.get("CFBundleIdentifier") != "org.orqwa.bike":
        found.append("macOS app must use ORQWA's bundle identifier")
    description = info.get("NSMicrophoneUsageDescription")
    if not isinstance(description, str) or not description.strip():
        found.append("macOS app needs a non-empty microphone usage description")
    if entitlements.get("com.apple.security.device.audio-input") is not True:
        found.append("macOS app signature needs the audio-input entitlement")
    return found


def main() -> None:
    bundle = pathlib.Path(sys.argv[1])
    with (bundle / "Contents" / "Info.plist").open("rb") as source:
        info = plistlib.load(source)
    signed = subprocess.run(
        ["codesign", "--display", "--entitlements", ":-", str(bundle)],
        check=True,
        capture_output=True,
    )
    entitlements = plistlib.loads(signed.stdout)
    found = problems(info, entitlements)
    if found:
        sys.exit("\n".join(found))
    print("macOS bundle microphone privacy checks passed")


if __name__ == "__main__":
    main()
