"""Dependency-free structural checks for the Terraform scaffold."""

from pathlib import Path
import sys


ROOT = Path(__file__).resolve().parents[1]
TERRAFORM = ROOT / "infra/terraform"
REQUIRED = ("versions.tf", "variables.tf", "main.tf", "outputs.tf", "terraform.tfvars.example")
MARKERS = (
    "aws_s3_bucket",
    "aws_sqs_queue",
    "aws_cloudwatch_log_group",
    "aws_iam_role",
    "aws_secretsmanager_secret",
    "aws_budgets_budget",
    "enable_rds",
    "publicly_accessible    = false",
)


def main() -> int:
    missing = [name for name in REQUIRED if not (TERRAFORM / name).is_file()]
    contents = "\n".join(path.read_text(encoding="utf-8") for path in TERRAFORM.glob("*.tf"))
    failures = [f"missing {name}" for name in missing]
    failures.extend(f"missing Terraform marker: {marker}" for marker in MARKERS if marker not in contents)
    forbidden = ("aws_access_key", "aws_secret_access_key", "BEGIN PRIVATE KEY")
    failures.extend(f"forbidden credential marker: {marker}" for marker in forbidden if marker in contents)
    if failures:
        print("Infrastructure checks failed:")
        print("\n".join(f" - {failure}" for failure in failures))
        return 1
    print("Infrastructure scaffold checks passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
