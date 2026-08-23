"""HTTP contract tests for the executable FastAPI boundary."""

import unittest
from uuid import UUID

from fastapi.testclient import TestClient

from api.app import app, reset_runtime


def sync_change(path: str = "D:/Footage/a.mp4") -> dict:
    return {
        "operation": "UPSERT",
        "asset": {
            "content_hash": "a" * 64,
            "size_bytes": 123,
            "metadata": {"container": "mp4"},
        },
        "local_file": {
            "path": path,
            "status": "ACTIVE",
            "observed_at": "2026-08-21T10:15:00Z",
        },
    }


class FastApiRuntimeTests(unittest.TestCase):
    def setUp(self) -> None:
        reset_runtime()
        self.client = TestClient(app)
        response = self.client.post(
            "/v1/projects",
            headers={"X-User-Id": "user-1"},
            json={"name": "Demo project"},
        )
        self.assertEqual(response.status_code, 201)
        self.project_id = response.json()["id"]
        self.assertIsInstance(UUID(self.project_id), UUID)

    def test_health_exposes_contract_version(self) -> None:
        response = self.client.get("/v1/health")
        self.assertEqual(response.status_code, 200)
        self.assertEqual(response.json(), {"status": "ok", "contract_version": "v1"})

    def test_sync_accepts_changes_and_repeating_key_is_idempotent(self) -> None:
        payload = {
            "project_id": self.project_id,
            "client_id": "laptop",
            "idempotency_key": "batch-1",
            "base_cursor": 0,
            "changes": [sync_change()],
        }
        first = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json=payload,
        )
        second = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json=payload,
        )
        self.assertEqual(first.status_code, 200)
        self.assertEqual(first.json(), {"next_cursor": 1, "accepted_changes": 1, "conflicts": []})
        self.assertEqual(second.status_code, 200)
        self.assertEqual(second.json(), first.json())

    def test_invalid_payload_is_rejected_before_sync(self) -> None:
        response = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json={
                "project_id": self.project_id,
                "client_id": "laptop",
                "idempotency_key": "bad",
                "base_cursor": 0,
                "changes": [{**sync_change(), "asset": {"content_hash": "not-a-hash"}}],
            },
        )
        self.assertEqual(response.status_code, 422)

    def test_wrong_owner_is_rejected(self) -> None:
        response = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-2"},
            json={
                "project_id": self.project_id,
                "client_id": "laptop",
                "idempotency_key": "owner-check",
                "base_cursor": 0,
                "changes": [],
            },
        )
        self.assertEqual(response.status_code, 403)

    def test_stale_cursor_is_actionable(self) -> None:
        base = {
            "project_id": self.project_id,
            "client_id": "laptop",
            "base_cursor": 0,
            "changes": [sync_change()],
        }
        accepted = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json={**base, "idempotency_key": "first"},
        )
        stale = self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json={**base, "idempotency_key": "second"},
        )
        self.assertEqual(accepted.status_code, 200)
        self.assertEqual(stale.status_code, 409)
        self.assertEqual(
            stale.json()["detail"],
            {"code": "STALE_CURSOR", "expected_cursor": 1, "received_cursor": 0},
        )


    def test_get_project_changes_returns_project_state(self) -> None:
        payload = {
            "project_id": self.project_id,
            "client_id": "laptop",
            "idempotency_key": "batch-changes",
            "base_cursor": 0,
            "changes": [sync_change()],
        }
        self.client.post(
            f"/v1/projects/{self.project_id}/sync",
            headers={"X-User-Id": "user-1"},
            json=payload,
        )
        response = self.client.get(
            f"/v1/projects/{self.project_id}/changes",
            headers={"X-User-Id": "user-1"},
        )
        self.assertEqual(response.status_code, 200)
        data = response.json()
        self.assertEqual(data["current_cursor"], 1)
        self.assertIn("D:/Footage/a.mp4", data["local_files"])


if __name__ == "__main__":
    unittest.main()
