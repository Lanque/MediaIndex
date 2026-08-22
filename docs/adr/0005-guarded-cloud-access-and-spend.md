# ADR 0005: Guard cloud access and spend before enabling workloads

- Status: accepted
- Date: 2026-08-21

## Context

MediaIndex has a credible path from a local desktop tool to shared cloud
knowledge and selected asynchronous processing. Cloud services can expose
private project data and incur costs even when the application is still an
experiment.

## Decision

Cloud access is project-scoped and least-privilege by default. Original footage
stays local unless a user explicitly requests a processing operation. Any
selected artifact access uses short-lived signed URLs. Terraform keeps ECS,
RDS, and budget notifications disabled until network, IAM, secret, rollback,
and budget prerequisites are reviewed.

## Consequences

- authorization and cost controls are part of the design, not deployment
  cleanup;
- S3 contains selected temporary artifacts only and applies lifecycle expiry;
- worker capacity starts at zero and queue redrive limits failures;
- AWS activation requires a reviewed plan and explicit apply authorization;
- local-only development remains the default while the cloud path matures.

## Rejected alternative

Automatically uploading every original and enabling always-on cloud workers
would increase privacy exposure and make development spend unpredictable.
