# Contributing to MediaIndex

MediaIndex is developed as a portfolio-quality engineering project. The repository should make the reasoning, trade-offs, and failure behavior visible—not only the happy-path code.

## Before starting

1. Find an existing issue or open a new one.
2. Confirm the scope and acceptance criteria.
3. Check the relevant architecture document and ADRs.
4. Create a branch from `main`.

## Branch naming

Use:

```text
<type>/<issue-number>-<short-slug>
```

Examples:

- `feat/3-media-discovery`
- `fix/5-hash-resume`
- `test/7-ci-foundation`
- `docs/14-architecture-records`
- `infra/12-aws-baseline`

Allowed types are `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, and `infra`.

## Commits

Use Conventional Commit style:

```text
type(scope): imperative summary
```

Examples:

- `feat(scanner): detect changed media files`
- `test(index): cover moved-file identity`
- `docs(architecture): explain sync ownership`

Keep commits focused. A commit should be understandable and revertible on its own.

## Pull requests

- Open a draft PR early when the work benefits from feedback.
- Link the issue with `Closes #N` when the PR fully completes it, or `Relates to #N` when it is partial.
- Explain the user-visible result and the engineering boundary changed.
- Include tests run and any known limitations.
- Call out data-model, security, cost, or failure-mode changes.
- Update documentation and ADRs in the same PR when behavior or a major decision changes.
- Keep `main` releasable and do not merge with failing required checks.

## Definition of done

Work is done when:

- acceptance criteria are met;
- automated tests cover the important behavior and failure cases;
- formatting, linting, type checks, and relevant builds pass;
- documentation reflects the current behavior;
- the PR has a clear reviewable diff;
- no secrets, production data, or unnecessarily large media files are committed.

## Documentation rule

The project plan is the product input. The repository is the living engineering record. If implementation diverges from the plan, explain why in the PR and record durable architectural decisions as an ADR.
