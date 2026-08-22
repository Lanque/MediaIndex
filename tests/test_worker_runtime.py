"""Failure-focused tests for the worker state machine."""

import unittest

from worker.runtime import (
    InMemoryQueue,
    JobState,
    JobStore,
    PermanentJobError,
    TransientJobError,
    Worker,
    WorkerCrash,
)


class WorkerRuntimeTests(unittest.TestCase):
    def make_job(self, store: JobStore, request_id: str = "request-1"):
        job = store.create_job(request_id, "generate_preview", {"path": "clip.mp4"})
        return job

    def test_job_creation_is_idempotent_and_transitions_are_explicit(self) -> None:
        store = JobStore()
        first = self.make_job(store)
        second = self.make_job(store)
        self.assertIs(first, second)
        self.assertEqual(first.state, JobState.PENDING)

        queue = InMemoryQueue()
        worker = Worker(store, queue, lambda job: None)
        worker.enqueue(first)
        worker.run_once(now=0)
        self.assertEqual(first.state, JobState.COMPLETED)
        self.assertEqual(worker.events[-1]["event"], "job_completed")

    def test_transient_failures_use_exponential_backoff(self) -> None:
        store = JobStore()
        job = self.make_job(store)
        queue = InMemoryQueue()
        calls = 0

        def handler(_job):
            nonlocal calls
            calls += 1
            if calls < 3:
                raise TransientJobError("temporary encoder outage")

        worker = Worker(store, queue, handler, retry_base_delay=2, max_attempts=3)
        worker.enqueue(job)
        worker.run_once(now=0)
        self.assertEqual(job.state, JobState.QUEUED)
        self.assertFalse(worker.run_once(now=1))
        worker.run_once(now=2)
        self.assertEqual(job.state, JobState.QUEUED)
        worker.run_once(now=6)
        self.assertEqual(job.state, JobState.COMPLETED)
        self.assertEqual(job.attempts, 3)

    def test_permanent_failure_moves_to_dead_letter_with_diagnostics(self) -> None:
        store = JobStore()
        job = self.make_job(store)
        queue = InMemoryQueue()
        worker = Worker(
            store,
            queue,
            lambda _job: (_ for _ in ()).throw(PermanentJobError("bad input")),
        )
        worker.enqueue(job)

        worker.run_once(now=0)

        self.assertEqual(job.state, JobState.DEAD_LETTERED)
        self.assertEqual(len(queue.dead_letters), 1)
        self.assertIn("bad input", queue.dead_letters[0].reason)
        self.assertEqual(worker.events[-1]["request_id"], "request-1")

    def test_duplicate_delivery_does_not_run_a_completed_job_twice(self) -> None:
        store = JobStore()
        job = self.make_job(store)
        queue = InMemoryQueue()
        calls = 0

        def handler(_job):
            nonlocal calls
            calls += 1

        worker = Worker(store, queue, handler)
        worker.enqueue(job)
        queue.publish(job.job_id)
        worker.run_once(now=0)
        worker.run_once(now=0)

        self.assertEqual(calls, 1)
        self.assertEqual(worker.events[-1]["event"], "duplicate_delivery_ignored")

    def test_worker_crash_is_recovered_after_visibility_timeout(self) -> None:
        store = JobStore()
        job = self.make_job(store)
        queue = InMemoryQueue(visibility_timeout=5)
        calls = 0

        def handler(_job):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise WorkerCrash("process terminated")

        worker = Worker(store, queue, handler)
        worker.enqueue(job)
        with self.assertRaises(WorkerCrash):
            worker.run_once(now=0)
        self.assertEqual(job.state, JobState.PROCESSING)
        self.assertFalse(worker.run_once(now=4))
        worker.run_once(now=5)
        self.assertEqual(job.state, JobState.COMPLETED)
        self.assertTrue(
            any(event["event"] == "stale_processing_recovered" for event in worker.events)
        )

    def test_shutdown_request_stops_the_poll_loop(self) -> None:
        store = JobStore()
        queue = InMemoryQueue()
        worker = Worker(store, queue, lambda _job: None)
        worker.request_shutdown()
        self.assertEqual(worker.run_until_shutdown(max_cycles=10), 0)


if __name__ == "__main__":
    unittest.main()
