# MediaIndex documentation

This directory is the project’s living design and engineering record.

## Start here

- [Project plan](project-plan.md) — scope, phases, build order, and non-goals.
- [Architecture](architecture.md) — boundaries, data ownership, synchronization, jobs, search, and reliability.
- [Roadmap](roadmap.md) — phase exit criteria mapped to GitHub issues.
- [Development workflow](development-workflow.md) — issue, branch, commit, PR, and review conventions.
- [Local index](local-index.md) — SQLite schema, hashing, identity, and interrupted-scan behavior.
- [Local search](local-search.md) — offline filters, unavailable files, and clip opening.
- [Explicit AI visual index](adr/0005-explicit-ai-visual-index.md) — sampled-frame analysis, embeddings, timestamps, and cost boundaries.
- [Cloud contracts](cloud-contracts.md) — versioned payloads, PostgreSQL boundaries, and RLS.
- [Cloud sync](cloud-sync.md) — cursors, idempotency, retries, and authorization.
- [Worker runtime](worker-runtime.md) — retries, visibility leases, dead letters, and crash recovery.
- [Security and cost](security-and-cost.md) — executable guardrails and AWS budget prerequisites.
- [AWS architecture](aws-architecture.md) — guarded Terraform scaling path and defaults.
- [Engineering status](engineering-status.md) — stacked PR order and known limitations.
- [ADRs](adr/) — durable architectural decisions.

## Documentation maintenance

Documentation changes should be reviewed with the implementation they describe. When a trade-off affects data ownership, identity, reliability, security, cost, or user-visible behavior, add or update an ADR.
