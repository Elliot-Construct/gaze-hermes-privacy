"""Tests for streaming bridge and stream registry."""

from __future__ import annotations

import pytest
from unittest.mock import AsyncMock, MagicMock

from gaze_privacy.runtime import StreamRegistry
from gaze_privacy.sidecar_client import StreamClient


class FakeStreamClient:
    def __init__(self):
        self.feed_calls = []
        self.open_called = False

    async def feed(self, kind: str, text: str):
        self.feed_calls.append((kind, text))
        # Return restored text
        return text.replace("<token>", "alice@example.invalid")


@pytest.fixture
def stream_registry():
    return StreamRegistry()


@pytest.fixture
def fake_stream_client():
    return FakeStreamClient()


def test_stream_registry_reserve_and_get_or_open(stream_registry, fake_stream_client):
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    key = stream_registry.reserve(namespace, client=fake_stream_client)

    assert key == ("default", "s1", "r1")

    stream = stream_registry.get_or_open(key)
    assert stream is fake_stream_client


def test_stream_registry_get_or_open_unknown_key_raises(stream_registry):
    with pytest.raises(KeyError):
        stream_registry.get_or_open(("unknown", "s1", "r1"))


def test_stream_registry_finish_and_abort(stream_registry, fake_stream_client):
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    key = stream_registry.reserve(namespace, client=fake_stream_client)

    stream_registry.finish_if_open(key)
    stream_registry.abort_if_open(key)

    # Should not raise


def test_stream_registry_release_removes_key(stream_registry, fake_stream_client):
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    key = stream_registry.reserve(namespace, client=fake_stream_client)

    stream_registry.release(key)

    with pytest.raises(KeyError):
        stream_registry.get_or_open(key)


def test_stream_registry_same_key_returns_same_client(stream_registry, fake_stream_client):
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    key1 = stream_registry.reserve(namespace, client=fake_stream_client)
    key2 = stream_registry.reserve(namespace, client=fake_stream_client)

    assert key1 == key2
    stream1 = stream_registry.get_or_open(key1)
    stream2 = stream_registry.get_or_open(key2)
    assert stream1 is stream2