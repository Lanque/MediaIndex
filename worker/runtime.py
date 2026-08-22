"""Reliable job runtime with an SQS-shaped queue seam.

The in-memory queue is deliberately small, but models the behaviors the SQS
adapter must preserve: visibility leases, duplicate delivery, delayed retry,
and a dead-letter path.
"""

from __future__ import annotations

import copy
import json
import logging
import time
from dataclasses import dataclass, field
from enum import Enum
from threading import Event
from typing import Any, Callable


class JobState(str, Enum):
    PENDING = "PENDING"
    QUEUED = "QUEUED"
    PROCESSING = "PROCESSING"
    COMPLETED = "COMPLETED"
    FAILED = "FAILED"
    DEAD_LETTERED = "DEAD_LETTERED"


_ALLOWED_TRANSITIONS: dict[JobState, set[JobState]] = {
    JobState.PENDING: {JobState.QUEUED},
    JobState.QUEUED: {JobState.PROCESSING},
    JobState.PROCESSING: {JobState.QUEUED, JobState.COMPLETED, JobState.FAILED},
    JobState.FAILED: {JobState.DEAD_LETTERED},
    JobState.COMPLETED: set(),
    JobState.DEAD_LETTERED: set(),
}


class WorkerError(RuntimeError):
    """Base class for job failures."""


class TransientJobError(WorkerError):
    """The job may succeed after a delayed retry."""


class PermanentJobError(WorkerError):
    """The job must be moved to the dead-letter path."""


class WorkerCrash(RuntimeError):
    """Test seam for a process crash after a message was received."""


class InvalidTransitionError(WorkerError):
    pass


@dataclass
class Job:
    job_id: str
    request_id: str
    operation: str
    payload: dict[str, Any]
    state: JobState = JobState.PENDING
    attempts: int = 0
    last_error: str | None = None


class JobStore:
    def __init__(self) -> None:
        self.jobs: dict[str, Job] = {}
        self._request_ids: dict[str, str] = {}
        self._next_id = 1

    def create_job(
        self, request_id: str, operation: str, payload: dict[str, Any]
    ) -> Job:
        existing_id = self._request_ids.get(request_id)
        if existing_id is not None:
            existing = self.jobs[existing_id]
            if existing.operation != operation or existing.payload != payload:
                raise WorkerError("request_id already refers to a different job")
            return existing

        job = Job(
            job_id=f"job-{self._next_id}",
            request_id=request_id,
            operation=operation,
            payload=copy.deepcopy(payload),
        )
        self._next_id += 1
        self.jobs[job.job_id] = job
        self._request_ids[request_id] = job.job_id
        return job

    def transition(
        self, job_id: str, state: JobState, error: str | None = None
    ) -> Job:
        job = self.jobs[job_id]
        if state not in _ALLOWED_TRANSITIONS[job.state]:
            raise InvalidTransitionError(
                f"cannot transition {job.job_id} from {job.state.value} to {state.value}"
            )
        job.state = state
        if error is not None:
            job.last_error = error
        return job


@dataclass
class QueueMessage:
    message_id: str
    job_id: str
    available_at: float = 0.0
    receive_count: int = 0


@dataclass
class DeadLetter:
    message_id: str
    job_id: str
    reason: str
    receive_count: int


class InMemoryQueue:
    def __init__(self, visibility_timeout: float = 30.0) -> None:
        self.visibility_timeout = visibility_timeout
        self.messages: dict[str, QueueMessage] = {}
        self.dead_letters: list[DeadLetter] = []
        self._next_id = 1

    def publish(self, job_id: str) -> QueueMessage:
        message = QueueMessage(f"message-{self._next_id}", job_id)
        self._next_id += 1
        self.messages[message.message_id] = message
        return message

    def receive(self, now: float) -> QueueMessage | None:
        for message in self.messages.values():
            if message.available_at <= now:
                message.receive_count += 1
                message.available_at = now + self.visibility_timeout
                return message
        return None

    def acknowledge(self, message_id: str) -> None:
        self.messages.pop(message_id, None)

    def release(self, message_id: str, available_at: float) -> None:
        self.messages[message_id].available_at = available_at

    def move_to_dead_letter(self, message_id: str, reason: str) -> None:
        message = self.messages.pop(message_id)
        self.dead_letters.append(
            DeadLetter(message.message_id, message.job_id, reason, message.receive_count)
        )


Handler = Callable[[Job], None]


class Worker:
    def __init__(
        self,
        store: JobStore,
        queue: InMemoryQueue,
        handler: Handler,
        *,
        max_attempts: int = 3,
        retry_base_delay: float = 1.0,
        retry_max_delay: float = 300.0,
        logger: logging.Logger | None = None,
    ) -> None:
        if max_attempts < 1:
            raise ValueError("max_attempts must be positive")
        self.store = store
        self.queue = queue
        self.handler = handler
        self.max_attempts = max_attempts
        self.retry_base_delay = retry_base_delay
        self.retry_max_delay = retry_max_delay
        self.logger = logger or logging.getLogger("mediaindex.worker")
        self.events: list[dict[str, Any]] = []
        self._shutdown = Event()

    def enqueue(self, job: Job) -> QueueMessage:
        self.store.transition(job.job_id, JobState.QUEUED)
        return self.queue.publish(job.job_id)

    def run_once(self, now: float | None = None) -> bool:
        timestamp = time.time() if now is None else now
        message = self.queue.receive(timestamp)
        if message is None:
            return False
        job = self.store.jobs[message.job_id]

        if job.state in {JobState.COMPLETED, JobState.DEAD_LETTERED}:
            self.queue.acknowledge(message.message_id)
            self._event("duplicate_delivery_ignored", job, message)
            return True

        if job.state == JobState.PROCESSING:
            self.store.transition(job.job_id, JobState.QUEUED)
            self._event("stale_processing_recovered", job, message)
        if job.state == JobState.QUEUED:
            self.store.transition(job.job_id, JobState.PROCESSING)
        else:
            raise InvalidTransitionError(
                f"queued message references job in {job.state.value} state"
            )

        job.attempts += 1
        self._event("job_started", job, message)
        try:
            self.handler(job)
        except WorkerCrash:
            self._event("worker_crashed", job, message)
            raise
        except PermanentJobError as error:
            self._dead_letter(job, message, str(error), timestamp)
        except TransientJobError as error:
            self._retry_or_dead_letter(job, message, str(error), timestamp)
        except Exception as error:  # unexpected errors are retryable by default
            self._retry_or_dead_letter(job, message, f"unexpected error: {error}", timestamp)
        else:
            self.store.transition(job.job_id, JobState.COMPLETED)
            self.queue.acknowledge(message.message_id)
            self._event("job_completed", job, message)
        return True

    def run_until_shutdown(self, max_cycles: int | None = None) -> int:
        cycles = 0
        while not self._shutdown.is_set() and (
            max_cycles is None or cycles < max_cycles
        ):
            cycles += 1
            if not self.run_once():
                break
        return cycles

    def request_shutdown(self) -> None:
        self._shutdown.set()

    def _retry_or_dead_letter(
        self, job: Job, message: QueueMessage, reason: str, now: float
    ) -> None:
        if job.attempts >= self.max_attempts:
            self._dead_letter(job, message, reason, now)
            return
        delay = min(
            self.retry_max_delay,
            self.retry_base_delay * (2 ** (job.attempts - 1)),
        )
        self.store.transition(job.job_id, JobState.QUEUED, error=reason)
        self.queue.release(message.message_id, now + delay)
        self._event("retry_scheduled", job, message, delay=delay, reason=reason)

    def _dead_letter(
        self, job: Job, message: QueueMessage, reason: str, now: float
    ) -> None:
        self.store.transition(job.job_id, JobState.FAILED, error=reason)
        self.queue.move_to_dead_letter(message.message_id, reason)
        self.store.transition(job.job_id, JobState.DEAD_LETTERED, error=reason)
        self._event("job_dead_lettered", job, message, reason=reason, at=now)

    def _event(
        self,
        event: str,
        job: Job,
        message: QueueMessage,
        **extra: Any,
    ) -> None:
        record = {
            "event": event,
            "job_id": job.job_id,
            "request_id": job.request_id,
            "state": job.state.value,
            "message_id": message.message_id,
            "attempt": job.attempts,
            **extra,
        }
        self.events.append(record)
        self.logger.info(json.dumps(record, sort_keys=True))
