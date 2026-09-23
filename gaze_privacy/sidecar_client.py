"""Sidecar REST and WebSocket client."""

from __future__ import annotations

import asyncio
import uuid
from dataclasses import dataclass
from typing import Any

import httpx
import websockets


@dataclass(frozen=True)
class SidecarStatus:
    protocol_version: int
    gaze_version: str
    ner_model: str | None = None


class SidecarClient:
    """HTTP client for the Gaze sidecar REST API."""

    def __init__(self, base_url: str, token: str):
        self.base_url = base_url.rstrip("/")
        self.headers = {"Authorization": f"Bearer {token}"}
        self._client = httpx.AsyncClient(headers=self.headers, timeout=30.0)

    async def close(self) -> None:
        await self._client.aclose()

    async def clean(self, namespace: dict[str, str], fields: list[dict[str, str]]) -> dict[str, Any]:
        response = await self._client.post(
            f"{self.base_url}/v1/clean",
            json={"namespace": namespace, "fields": fields},
        )
        response.raise_for_status()
        return response.json()

    async def restore(self, namespace: dict[str, str], fields: list[dict[str, str]]) -> dict[str, Any]:
        response = await self._client.post(
            f"{self.base_url}/v1/restore",
            json={"namespace": namespace, "fields": fields},
        )
        response.raise_for_status()
        return response.json()

    async def status(self) -> SidecarStatus:
        response = await self._client.get(f"{self.base_url}/v1/status")
        response.raise_for_status()
        data = response.json()
        return SidecarStatus(
            protocol_version=data["protocol_version"],
            gaze_version=data["gaze_version"],
            ner_model=data.get("ner_model"),
        )

    async def validate_policy(self, toml: str) -> dict[str, Any]:
        response = await self._client.post(
            f"{self.base_url}/v1/policies/validate",
            json={"toml": toml},
        )
        response.raise_for_status()
        return response.json()

    async def edit_policy(self, scope: str, edit: dict[str, Any]) -> dict[str, Any]:
        response = await self._client.post(
            f"{self.base_url}/v1/policies/edit",
            json={"scope": scope, "edit": edit},
        )
        response.raise_for_status()
        return response.json()

    async def apply_policy(self, scope: str, expected_hash: str, toml: str) -> dict[str, Any]:
        response = await self._client.post(
            f"{self.base_url}/v1/policies/apply",
            json={"scope": scope, "expected_hash": expected_hash, "toml": toml},
        )
        response.raise_for_status()
        return response.json()

    async def effective_policy(self, profile_id: str) -> dict[str, Any]:
        response = await self._client.get(
            f"{self.base_url}/v1/policies/effective",
            params={"profile_id": profile_id},
        )
        response.raise_for_status()
        return response.json()

    async def list_sessions(self) -> dict[str, Any]:
        response = await self._client.get(f"{self.base_url}/v1/sessions")
        response.raise_for_status()
        return response.json()

    async def get_session(self, profile_id: str, session_id: str) -> dict[str, Any]:
        response = await self._client.get(f"{self.base_url}/v1/sessions/{profile_id}/{session_id}")
        response.raise_for_status()
        return response.json()

    async def recover_session(self, profile_id: str, session_id: str) -> dict[str, Any]:
        response = await self._client.post(f"{self.base_url}/v1/sessions/{profile_id}/{session_id}/recover")
        response.raise_for_status()
        return response.json()

    async def delete_session(self, profile_id: str, session_id: str) -> None:
        response = await self._client.delete(f"{self.base_url}/v1/sessions/{profile_id}/{session_id}")
        response.raise_for_status()

    async def metrics(self) -> dict[str, Any]:
        response = await self._client.get(f"{self.base_url}/v1/metrics")
        response.raise_for_status()
        return response.json()

    def open_stream(self, namespace: dict[str, str]) -> "StreamClient":
        stream_id = uuid.uuid4().hex
        ws_url = (
            self.base_url.replace("http://", "ws://", 1).replace("https://", "wss://", 1)
            + "/v1/streams/"
            + stream_id
        )
        return StreamClient.connect(ws_url, self.headers, namespace)


class StreamClient:
    """WebSocket client for streaming restoration."""

    def __init__(self, ws_url: str, headers: dict[str, str], namespace: dict[str, str]):
        self.ws_url = ws_url
        self.headers = headers
        self.namespace = namespace
        self._ws: websockets.ClientConnection | None = None
        self._cm: Any = None
        self._seq = 0
        self._lock = asyncio.Lock()

    @classmethod
    async def connect(
        cls, ws_url: str, headers: dict[str, str], namespace: dict[str, str]
    ) -> "StreamClient":
        client = cls(ws_url, headers, namespace)
        cm = websockets.connect(ws_url, extra_headers=headers)
        client._cm = cm
        client._ws = await cm.__aenter__()
        # Send open message
        await client._ws.send(
            '{"type":"open","namespace":' + str(namespace).replace("'", '"') + "}"
        )
        return client

    async def feed(self, kind: str, text: str) -> str:
        if self._ws is None:
            raise RuntimeError("Stream not connected")
        async with self._lock:
            self._seq += 1
            msg = {"type": "chunk", "seq": self._seq, "kind": kind, "text": text}
            await self._ws.send(str(msg).replace("'", '"'))
            response = await self._ws.recv()
            # Parse response - expect {"seq": N, "text": "restored"}
            import json

            data = json.loads(response)
            return data.get("text", "")

    async def finish(self) -> str:
        if self._ws is None:
            raise RuntimeError("Stream not connected")
        async with self._lock:
            self._seq += 1
            msg = {"type": "finish", "seq": self._seq}
            await self._ws.send(str(msg).replace("'", '"'))
            response = await self._ws.recv()
            import json

            data = json.loads(response)
            return data.get("text", "")

    async def abort(self) -> None:
        if self._ws is None:
            return
        async with self._lock:
            self._seq += 1
            msg = {"type": "abort", "seq": self._seq}
            await self._ws.send(str(msg).replace("'", '"'))
        if self._cm is not None:
            await self._cm.__aexit__(None, None, None)
        self._ws = None
        self._cm = None

    async def __aenter__(self) -> "StreamClient":
        return self

    async def __aexit__(self, exc_type, exc_val, exc_tb) -> None:
        await self.abort()