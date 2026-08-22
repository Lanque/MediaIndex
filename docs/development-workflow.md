# Development workflow

## Issue first

Every change starts with an issue. Use the issue to state:

- the problem or user value;
- scope and non-scope;
- acceptance criteria;
- test expectations;
- security, cost, and failure-mode implications.

Epic issues describe a phase. Task issues describe one reviewable slice of work. Documentation and ADR issues keep the engineering story explicit.

## Branches

```text
main
  └── <type>/<issue-number>-<short-slug>
```

Keep `main` stable. Use one branch per issue or tightly coupled issue set. Do not develop directly on `main`.

## Commits

Prefer small commits that tell a sequence:

1. establish a model or interface;
2. implement the happy path;
3. add failure handling;
4. add tests;
5. update docs.

Use Conventional Commit prefixes and include the issue number in the PR even when it is not repeated in every commit.

## Pull requests

Open a draft PR when the boundary or design benefits from early feedback. A PR should explain:

- what changed and why;
- which issue(s) it addresses;
- what was tested;
- what remains deliberately out of scope;
- how failure, security, and cost behavior changed.

Use the PR template. Keep documentation, migrations, tests, and code in the same review unit when they describe one behavior.

## Review and merge

A PR is ready when acceptance criteria are checked, required CI is green, the diff is focused, and the documentation is current. Squash merging is preferred for a clean portfolio history; the PR title should remain a useful summary.

## Release and deployment posture

Until the AWS and hardening phases are complete, local development is the primary supported environment. Do not describe an unverified deployment as production-ready. Infrastructure changes require explicit cost and rollback notes.
