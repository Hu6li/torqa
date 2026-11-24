"""A release needs the maintainers' roots and the real activation server (ADR 0043)."""

import unittest

import licensing

READY_TRUST = 'const RELEASE_ROOTS: &[&str] = &[\n    r#"{"ed25519": "…"}"#,\n];\n'
READY_SERVER = 'pub const SERVER: &str = "https://activate.example.org";\n'


class Release(unittest.TestCase):
    def test_roots_and_a_real_server_may_be_released(self):
        self.assertEqual(licensing.problems(READY_TRUST, READY_SERVER), [])

    def test_no_roots_cannot_be_released(self):
        found = licensing.problems("const RELEASE_ROOTS: &[&str] = &[];\n", READY_SERVER)
        self.assertEqual(len(found), 1)
        self.assertIn("RELEASE_ROOTS", found[0])

    def test_the_placeholder_server_cannot_be_released(self):
        for server in ("https://activate.orqwa.invalid", "http://activate.example.org"):
            found = licensing.problems(READY_TRUST, f'pub const SERVER: &str = "{server}";\n')
            self.assertEqual(len(found), 1, server)


if __name__ == "__main__":
    unittest.main()
