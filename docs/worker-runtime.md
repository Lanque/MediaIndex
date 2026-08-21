# Worker runtime and failure handling

The worker runtime in [`worker/runtime.py`](../worker/runtime.py) models the
durability contract required from the future SQS adapter.

## State machine

```text
PENDING -> QUEUED -> PROCESSING -> COMPLETED
                              |
                              +-> QUEUED (transient retry)
                              +-> FAILED -> DEAD_LETTERED
```

Each job has a stable `job_id` and caller `request_id`. Reusing a request ID
with the same operation/payload returns the existing job; reusing it for a
different operation is rejected.

## Delivery and retry behavior

The queue uses a visibility lease. A worker crash after receive leaves the
message unacknowledged; after the lease expires, another worker can recover a
stale `PROCESSING` job. A completed or dead-lettered job acknowledges duplicate
delivery without running the handler again.

Transient failures use exponential backoff capped by a maximum delay. A
permanent failure, or a transient failure reaching the attempt limit, records
the diagnostic and moves the message to the dead-letter path. Structured event
records always include `job_id`, `request_id`, state, message ID, and attempt.

The in-memory queue is a test seam. The production SQS adapter must preserve
the same visibility, acknowledgement, retry, and dead-letter semantics.

The first explicit operation using this runtime is the selected-media preview
job documented in [docs/preview-job.md](preview-job.md).
