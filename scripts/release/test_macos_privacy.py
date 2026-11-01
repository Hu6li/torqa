"""Check the actual signed artifact's privacy contract, rather than the preset alone."""

import unittest

from macos_privacy import problems


class MacOSPrivacyTests(unittest.TestCase):
    def setUp(self):
        self.info = {
            "CFBundleIdentifier": "org.orqwa.bike",
            "NSMicrophoneUsageDescription": "Test your microphone or speak in group voice.",
        }
        self.entitlements = {"com.apple.security.device.audio-input": True}

    def test_complete_bundle_can_request_as_orqwa(self):
        self.assertEqual(problems(self.info, self.entitlements), [])

    def test_missing_entitlement_reproduces_unpromptable_artifact(self):
        for value in (None, False, "true", 1):
            with self.subTest(value=value):
                self.assertIn("audio-input", problems(
                    self.info, {"com.apple.security.device.audio-input": value}
                )[0])

    def test_absent_or_empty_usage_description_cannot_request_safely(self):
        for value in (None, "", "  ", True):
            with self.subTest(value=value):
                info = self.info | {"NSMicrophoneUsageDescription": value}
                self.assertIn("usage description", problems(info, self.entitlements)[0])

    def test_settings_entry_belongs_to_the_app(self):
        self.info["CFBundleIdentifier"] = "org.godotengine.godot"
        self.assertIn("bundle identifier", problems(self.info, self.entitlements)[0])
