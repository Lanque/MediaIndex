"""Validate migration naming, ordering, and security markers without a DB."""

from pathlib import Path
import re
import sys


ROOT = Path(__file__).resolve().parents[1]
MIGRATION_NAME = re.compile(r"^(\d{3})_[a-z0-9_]+\.sql$")


def main() -> int:
    directory = ROOT / "migrations"
    files = sorted(path for path in directory.glob("*.sql") if not path.name.endswith(".down.sql"))
    numbers = []
    failures = []
    for path in files:
        match = MIGRATION_NAME.match(path.name)
        if not match:
            failures.append(f"invalid migration filename: {path.name}")
            continue
        numbers.append(int(match.group(1)))
        contents = path.read_text(encoding="utf-8").upper()
        for marker in ("BEGIN;", "COMMIT;", "ROW LEVEL SECURITY", "CREATE POLICY"):
            if marker not in contents:
                failures.append(f"{path.name} is missing {marker}")

    expected = list(range(1, len(numbers) + 1))
    if numbers != expected:
        failures.append(f"migration numbers must be contiguous from 001: {numbers}")

    if failures:
        print("Migration checks failed:")
        print("\n".join(f" - {failure}" for failure in failures))
        return 1
    print(f"Migration checks passed ({len(files)} migration files).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
