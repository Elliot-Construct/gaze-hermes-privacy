"""Tests for temporary sensitive reveal grants."""

from __future__ import annotations

import pytest
import time
from unittest.mock import MagicMock

from gaze_privacy.reveal import RevealService, RevealGrant, RevealExpired, RevealMissing, RevealProfileMismatch


class FakeClock:
    def __init__(self, initial=1000.0):
        self._time = initial

    def __call__(self):
        return self._time

    def advance(self, seconds: float):
        self._time += seconds


@pytest.fixture
def clock():
    return FakeClock()


@pytest.fixture
def reveal_service(clock):
    return RevealService(clock=clock)


@pytest.fixture
def sample_event():
    return {"id": "evt123", "original": "synthetic@example.invalid", "profile_id": "default"}


def test_reveal_token_expires_and_never_enters_event_log(reveal_service, clock, sample_event):
    event_id = sample_event["id"]
    # add_sanitised should receive sanitized event (no sensitive data)
    sanitized_event = {"id": event_id, "profile_id": "default"}
    reveal_service.events.add_sanitised(sanitized_event)
    grant = reveal_service.issue(event_id, ttl_seconds=60)

    assert reveal_service.read(grant.token)["original"] == "synthetic@example.invalid"

    clock.advance(61)
    with pytest.raises(RevealExpired):
        reveal_service.read(grant.token)

    assert "synthetic@example.invalid" not in reveal_service.events.serialized_log()


def test_reveal_profile_mismatch_rejected(reveal_service, sample_event):
    event_id = sample_event["id"]
    # Use sanitized event for add_sanitised
    sanitized_event = {"id": event_id, "profile_id": "default"}
    reveal_service.events.add_sanitised(sanitized_event)
    grant = reveal_service.issue(event_id, profile_id="default")

    with pytest.raises(RevealProfileMismatch, match="[Pp]rofile"):
        reveal_service.read(grant.token, profile_id="other")


def test_reveal_single_use_consumed(reveal_service, sample_event):
    event_id = sample_event["id"]
    reveal_service.events.add_sanitised(sample_event)
    grant = reveal_service.issue(event_id)

    # First read works
    result1 = reveal_service.read(grant.token)
    assert result1["original"] == "synthetic@example.invalid"

    # Second read fails
    with pytest.raises(Exception, match="consumed"):
        reveal_service.read(grant.token)


def test_reveal_missing_token_rejected(reveal_service):
    with pytest.raises(RevealMissing, match="not found"):
        reveal_service.read("nonexistent-token")


def test_reveal_ttl_capped_at_60_seconds(reveal_service, clock):
    event_id = "evt123"
    reveal_service.events.add_sanitised({"id": event_id, "original": "test", "profile_id": "default"})

    # Request 120 seconds, should be capped at 60
    grant = reveal_service.issue(event_id, ttl_seconds=120)
    assert grant.expires_at - clock() <= 61  # ~60 seconds


def test_reveal_minimum_ttl_1_second(reveal_service, clock):
    event_id = "evt123"
    reveal_service.events.add_sanitised({"id": event_id, "original": "test", "profile_id": "default"})

    # Request 0 seconds, should be at least 1
    grant = reveal_service.issue(event_id, ttl_seconds=0)
    assert grant.expires_at - clock() >= 1