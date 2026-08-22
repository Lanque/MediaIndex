# Explicit preview job

Preview generation is the first selected-media operation in the worker path.
Indexing never creates this job automatically. A caller submits a
`generate_preview` job with a selected source and output location; the worker
validates both paths and then invokes the testable FFmpeg adapter.

The worker runtime supplies idempotent request handling, duplicate-delivery
protection, retries, dead-letter diagnostics, and structured job identifiers.
The adapter only owns FFmpeg command construction and output verification, so a
future S3 materializer can replace the local source adapter without changing
job-state behavior.

The current tests use synthetic bytes and a fake command runner. A real media
fixture or FFmpeg installation is not required for CI.
