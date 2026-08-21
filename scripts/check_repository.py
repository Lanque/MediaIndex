"""Fast, dependency-free checks for the repository foundation."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]

REQUIRED_FILES = (
    "README.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "docs/README.md",
    "docs/project-plan.md",
    "docs/architecture.md",
    "docs/development-workflow.md",
    "docs/roadmap.md",
    "docs/adr/0001-content-hash-as-media-identity.md",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/ISSUE_TEMPLATE/bug_report.md",
)


def main() -> int:
    missing = [path for path in REQUIRED_FILES if not (ROOT / path).is_file()]
    if missing:
        print("Missing required repository files:")
        print("\n".join(f" - {path}" for path in missing))
        return 1

    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    architecture = (ROOT / "docs/architecture.md").read_text(encoding="utf-8")
    roadmap = (ROOT / "docs/roadmap.md").read_text(encoding="utf-8")

    checks = {
        "README identifies MediaIndex": "MediaIndex" in readme,
        "architecture defines SQLite ownership": "SQLite" in architecture,
        "architecture defines PostgreSQL ownership": "PostgreSQL" in architecture,
        "roadmap references Phase 1": "Phase 1" in roadmap,
        "roadmap references issue 1": "issues/1" in roadmap,
        "roadmap references issue 7": "issues/7" in roadmap,
    }

    failed = [name for name, passed in checks.items() if not passed]
    if failed:
        print("Repository foundation checks failed:")
        print("\n".join(f" - {name}" for name in failed))
        return 1

    print(f"Repository foundation checks passed ({len(REQUIRED_FILES)} required files).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
