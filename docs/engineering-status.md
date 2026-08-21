# Engineering status

The repository is intentionally developed as a stacked, issue-linked series
of draft pull requests. The current implementation order is:

| Area | Issue/PR | Status |
| --- | --- | --- |
| Tauri shell and CI foundation | #2, #7 / #16–#18 | draft PRs |
| Discovery and change detection | #3 / #20 | draft PR, CI green |
| FFprobe metadata | #4 / #21 | draft PR, CI green |
| SQLite local index | #5 / #22 | draft PR, CI green |
| Offline search and opening | #6 / #23 | draft PR, CI green |
| Cloud contracts and PostgreSQL model | #9 / #24 | draft PR, CI green |
| Cursor/idempotent sync | #10 / #25 | draft PR, CI green |
| Worker retries and dead letters | #11 / #26 | draft PR, CI green |
| Security guardrails | #13 / #27 | draft PR, CI running or green |
| AWS/Terraform scaffold | #12 / #28 | draft PR, static checks green |
| Documentation maintenance | #14 / this PR | in review |
| First selected-media preview job | #30 / #32 | draft PR, local checks green |
| FastAPI sync runtime | #31 / next PR | implementation in progress |

## Deliberate limitations

- Pull requests remain drafts until the owner reviews and explicitly merges
  the stack. The intended merge order is the table order from local work
  through infrastructure.
- GitHub main-branch protection is documented in
  [docs/github-repository-settings.md](github-repository-settings.md), but the
  private repository plan returned GitHub HTTP 403 because branch protection
  requires GitHub Pro or a public repository.
- Terraform CLI and a PostgreSQL server are not required for the default CI;
  static migration/infrastructure checks cover structure, while operators must
  run `terraform validate` and `psql`-based checks in their deployment
  environment before applying anything.
- AWS infrastructure has not been applied and no credentials are stored in the
  repository.
