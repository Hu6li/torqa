"""A release needs the maintainers' roots built in (ADR 0043)."""

import unittest

import licensing

READY_TRUST = 'const RELEASE_ROOTS: &[&str] = &[\n    r#"{"ed25519": "…"}"#,\n];\n'


class Release(unittest.TestCase):
    def test_roots_may_be_released(self):
        self.assertEqual(licensing.problems(READY_TRUST), [])

    def test_no_roots_cannot_be_released(self):
        found = licensing.problems("const RELEASE_ROOTS: &[&str] = &[];\n")
        self.assertEqual(len(found), 1)
        self.assertIn("RELEASE_ROOTS", found[0])


if __name__ == "__main__":
    unittest.main()
