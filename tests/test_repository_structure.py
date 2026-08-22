"""Unit tests for the documentation and repository foundation."""

from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]


class RepositoryStructureTests(unittest.TestCase):
    def test_required_directories_exist(self) -> None:
        for relative_path in ("docs", "docs/adr", ".github", ".github/ISSUE_TEMPLATE"):
            with self.subTest(relative_path=relative_path):
                self.assertTrue((ROOT / relative_path).is_dir())

    def test_adr_sequence_starts_with_content_identity(self) -> None:
        adr = ROOT / "docs/adr/0001-content-hash-as-media-identity.md"
        contents = adr.read_text(encoding="utf-8")
        self.assertIn("content hash", contents.lower())
        self.assertIn("MediaAsset", contents)

    def test_issue_and_pull_request_templates_exist(self) -> None:
        self.assertTrue((ROOT / ".github/pull_request_template.md").is_file())
        self.assertTrue((ROOT / ".github/ISSUE_TEMPLATE/feature_request.md").is_file())
        self.assertTrue((ROOT / ".github/ISSUE_TEMPLATE/bug_report.md").is_file())
        self.assertTrue((ROOT / ".github/ISSUE_TEMPLATE/architecture_decision.md").is_file())

    def test_roadmap_covers_first_phase_tasks(self) -> None:
        roadmap = (ROOT / "docs/roadmap.md").read_text(encoding="utf-8")
        for issue_number in range(1, 8):
            with self.subTest(issue_number=issue_number):
                self.assertIn(f"issues/{issue_number}", roadmap)


if __name__ == "__main__":
    unittest.main()
