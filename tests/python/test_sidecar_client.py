"""Tests for sidecar REST and WebSocket client."""

from __future__ import annotations

import asyncio
import json
from contextlib import asynccontextmanager
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from gaze_privacy.sidecar_client import SidecarClient, StreamClient, SidecarStatus


@pytest.mark.asyncio
async def test_clean_posts_to_v1_clean_with_bearer_auth():
    client = SidecarClient("http://127.0.0.1:65113", "test-token")

    with patch.object(client._client, "post", new_callable=AsyncMock) as mock_post:
        mock_response = MagicMock()
        mock_response.json.return_value = {
            "fields": [{"path": "/messages/0/content", "text": "Contact <token>"}],
            "detections": [{"class": "email", "count": 1}],
            "policy_version": "sha256:abc123",
        }
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        result = await client.clean(
            {"profile_id": "default", "session_id": "s1", "request_id": "r1"},
            [{"path": "/messages/0/content", "text": "Contact alice@example.invalid"}],
        )

        assert result["fields"][0]["text"] == "Contact <token>"
        mock_post.assert_called_once()
        call_args = mock_post.call_args
        assert call_args[0][0] == "http://127.0.0.1:65113/v1/clean"
        # The client adds headers to the request; check the actual call
        assert call_args[1]["json"]["namespace"]["profile_id"] == "default"


@pytest.mark.asyncio
async def test_restore_posts_to_v1_restore():
    client = SidecarClient("http://127.0.0.1:65113", "test-token")

    with patch.object(client._client, "post", new_callable=AsyncMock) as mock_post:
        mock_response = MagicMock()
        mock_response.json.return_value = {"fields": [{"path": "/messages/0/content", "text": "Contact alice@example.invalid"}]}
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        result = await client.restore(
            {"profile_id": "default", "session_id": "s1", "request_id": "r1"},
            [{"path": "/messages/0/content", "text": "Contact <token>"}],
        )

        assert result["fields"][0]["text"] == "Contact alice@example.invalid"
        call_args = mock_post.call_args
        assert call_args[0][0] == "http://127.0.0.1:65113/v1/restore"


@pytest.mark.asyncio
async def test_status_gets_v1_status():
    client = SidecarClient("http://127.0.0.1:65113", "test-token")

    with patch.object(client._client, "get", new_callable=AsyncMock) as mock_get:
        mock_response = MagicMock()
        mock_response.json.return_value = {
            "protocol_version": 1,
            "gaze_version": "0.14.0",
            "ner_model": "kiji-distilbert",
        }
        mock_response.raise_for_status = MagicMock()
        mock_get.return_value = mock_response

        result = await client.status()

        assert result.protocol_version == 1
        assert result.gaze_version == "0.14.0"
        call_args = mock_get.call_args
        assert call_args[0][0] == "http://127.0.0.1:65113/v1/status"


@asynccontextmanager
async def mock_websockets_connect(url, extra_headers=None):
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.recv = AsyncMock(return_value='{"seq":1,"text":"ok"}')
    mock_ws.close = AsyncMock()
    yield mock_ws


@pytest.mark.asyncio
async def test_open_stream_returns_stream_client():
    client = SidecarClient("http://127.0.0.1:65113", "test-token")

    with patch("websockets.connect", mock_websockets_connect):
        stream = await client.open_stream({"profile_id": "default", "session_id": "s1", "request_id": "r1"})

        assert isinstance(stream, StreamClient)
        assert stream.headers["Authorization"] == "Bearer test-token"


@asynccontextmanager
async def mock_websockets_connect_feed(url, extra_headers=None):
    mock_ws = AsyncMock()
    mock_ws.send = AsyncMock()
    mock_ws.recv = AsyncMock(side_effect=['{"seq":1,"text":"restored"}', '{"seq":2,"text":"restored"}'])
    mock_ws.close = AsyncMock()
    yield mock_ws


@pytest.mark.asyncio
async def test_stream_client_feed_serializes_under_lock():
    with patch("websockets.connect", mock_websockets_connect_feed):
        stream = await StreamClient.connect(
            "ws://127.0.0.1:65113/v1/streams/test",
            {"Authorization": "Bearer test"},
            {"profile_id": "default", "session_id": "s1", "request_id": "r1"},
        )

        result1 = await stream.feed("text", "Hello")
        result2 = await stream.feed("text", " world")

        assert result1 == "restored"
        assert result2 == "restored"
        assert stream._ws.send.call_count == 3  # open + 2 feeds