"""Tests for the dependency-free migration validator."""

import unittest

import scripts.check_migrations as check_migrations


class MigrationTests(unittest.TestCase):
    def test_migrations_are_numbered_and_security_checked(self) -> None:
        self.assertEqual(check_migrations.main(), 0)


if __name__ == "__main__":
    unittest.main()
