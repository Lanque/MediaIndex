"""Integration-style tests for cursor and retry behavior."""

import unittest
from uuid import uuid4

from api.sync import (
    AuthorizationError,
    InMemorySyncServer,
    StaleCursorError,
    SyncClient,
)


def change(path: str, content_hash: str = "a" * 64) -> dict:
    return {
        "operation": "UPSERT",
        "asset": {
            "content_hash": content_hash,
            "size_bytes": 123,
            "metadata": {"container": "mp4"},
        },
        "local_file": {
            "path": path,
            "status": "ACTIVE",
            "observed_at": "2026-08-21T10:15:00Z",
        },
    }


class CloudSyncTests(unittest.TestCase):
    def setUp(self) -> None:
        self.project_id = str(uuid4())
        self.server = InMemorySyncServer()
        self.server.create_project(self.project_id, "user-1")

    def test_client_advances_cursor_and_repeating_request_is_idempotent(self) -> None:
        client = SyncClient(self.project_id, "laptop")
        client.queue_change(change("D:/Footage/a.mp4"))

        response = client.sync(self.server.sync, "user-1")
        self.assertEqual(response["next_cursor"], 1)
        self.assertEqual(client.pending_count, 0)
        self.assertEqual(len(self.server.snapshot(self.project_id, "user-1")["assets"]), 1)

        request = {
            "project_id": self.project_id,
            "client_id": "laptop",
            "idempotency_key": "already-seen",
            "base_cursor": 1,
            "changes": [change("D:/Footage/b.mp4", "b" * 64)],
        }
        first = self.server.sync(request, "user-1")
        second = self.server.sync(request, "user-1")
        self.assertEqual(first, second)
        self.assertEqual(len(self.server.snapshot(self.project_id, "user-1")["local_files"]), 2)

    def test_network_interruption_retries_the_same_batch_and_resumes(self) -> None:
        client = SyncClient(self.project_id, "laptop")
        client.queue_change(change("D:/Footage/a.mp4"))
        client.queue_change(change("D:/Footage/b.mp4", "b" * 64))
        calls = 0

        def flaky_transport(request: dict, user_id: str) -> dict:
            nonlocal calls
            calls += 1
            if calls == 1:
                self.server.sync(request, user_id)
                raise ConnectionError("connection lost after server commit")
            return self.server.sync(request, user_id)

        with self.assertRaises(ConnectionError):
            client.sync(flaky_transport, "user-1", batch_size=2)
        self.assertEqual(client.pending_count, 2)

        response = client.sync(flaky_transport, "user-1", batch_size=2)
        self.assertEqual(response["accepted_changes"], 2)
        self.assertEqual(client.pending_count, 0)
        self.assertEqual(calls, 2)
        self.assertEqual(self.server.snapshot(self.project_id, "user-1")["cursor"], 1)

    def test_stale_cursor_and_wrong_owner_are_actionable(self) -> None:
        request = {
            "project_id": self.project_id,
            "client_id": "laptop",
            "idempotency_key": "first",
            "base_cursor": 0,
            "changes": [change("D:/Footage/a.mp4")],
        }
        self.server.sync(request, "user-1")
        stale = dict(request, idempotency_key="second")
        with self.assertRaises(StaleCursorError):
            self.server.sync(stale, "user-1")

        with self.assertRaises(AuthorizationError):
            self.server.snapshot(self.project_id, "user-2")


if __name__ == "__main__":
    unittest.main()
