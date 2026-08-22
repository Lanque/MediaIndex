"""Tests for the cost- and security-conscious infrastructure scaffold."""

import unittest

import scripts.check_infra as check_infra


class InfrastructureTests(unittest.TestCase):
    def test_terraform_scaffold_has_required_resources_and_no_credentials(self) -> None:
        self.assertEqual(check_infra.main(), 0)


if __name__ == "__main__":
    unittest.main()
