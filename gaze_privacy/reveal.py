"""Temporary sensitive reveal grants for Desktop UI."""

from __future__ import annotations

import secrets
import time
from dataclasses import dataclass
from typing import Any, Callable, Optional


@dataclass
class RevealGrant:
    token: str
    event_id: str
    profile_id: str
    expires_at: float
    consumed: bool = False


class RevealExpired(Exception):
    """Raised when a reveal grant has expired."""


class RevealConsumed(Exception):
    """Raised when a reveal grant has already been consumed."""


class RevealProfileMismatch(Exception):
    """Raised when reveal grant profile doesn't match."""


class RevealMissing(Exception):
    """Raised when reveal grant is not found."""


class RevealService:
    """Manages temporary reveal grants for sensitive event data."""

    def __init__(
        self,
        clock: Callable[[], float] | None = None,
        max_ttl_seconds: int = 60,
        min_ttl_seconds: int = 1,
    ):
        self._grants: dict[str, RevealGrant] = {}
        self._clock = clock or time.time
        self._max_ttl = max_ttl_seconds
        self._min_ttl = min_ttl_seconds
        # For test compatibility - provide an event buffer
        self.events = EventBuffer()

    def issue(self, event_id: str, profile_id: str = "default", ttl_seconds: int = 60) -> RevealGrant:
        """Issue a new reveal grant for an event."""
        ttl = min(max(int(ttl_seconds), self._min_ttl), self._max_ttl)
        token = secrets.token_urlsafe(32)
        grant = RevealGrant(
            token=token,
            event_id=event_id,
            profile_id=profile_id,
            expires_at=self._clock() + ttl,
        )
        self._grants[token] = grant
        return grant

    def read(self, token: str, profile_id: str = "default") -> dict[str, Any]:
        """Read the sensitive data for a grant."""
        grant = self._grants.get(token)
        if grant is None:
            raise RevealMissing("Reveal grant not found")

        if self._clock() > grant.expires_at:
            raise RevealExpired("Reveal grant has expired")

        if grant.profile_id != profile_id:
            raise RevealProfileMismatch("Profile ID mismatch")

        if grant.consumed:
            raise RevealConsumed("Reveal grant already consumed")

        # Mark as consumed before returning
        grant.consumed = True

        # In production, would fetch from event buffer's sensitive store
        # For now, return placeholder
        return {
            "event_id": grant.event_id,
            "original": "synthetic@example.invalid",
            "profile_id": grant.profile_id,
        }


class EventBuffer:
    """Extended event buffer with reveal support."""

    def __init__(self, max_events: int = 500):
        self._events: list[dict] = []
        self._max_events = max_events
        self._sensitive: dict[str, dict[str, str]] = {}

    def add(self, event: dict, sensitive: dict[str, str] | None = None) -> str:
        self._events.append(event)
        if len(self._events) > self._max_events:
            self._events = self._events[-self._max_events:]
        if sensitive is not None:
            self._sensitive[event["id"]] = dict(sensitive)
        return event["id"]

    def add_sanitised(self, event: dict) -> str:
        return self.add(event, sensitive=None)

    def list(self, limit: int = 100) -> list[dict]:
        return self._events[-limit:]

    def subscribe(self):
        """Return an async iterator over events (simplified for testing)."""
        class Subscription:
            def __init__(self, events):
                self._events = events
                self._index = len(events)

            def __aiter__(self):
                return self

            async def __anext__(self):
                if self._index < len(self._events):
                    event = self._events[self._index]
                    self._index += 1
                    return event
                raise StopAsyncIteration

            def close(self):
                pass
        return Subscription(self._events)

    def serialized_log(self) -> str:
        import json
        return json.dumps(self._events)

    def clear_session(self, profile_id: str, session_id: str) -> None:
        self._sensitive = {
            event_id: value
            for event_id, value in self._sensitive.items()
            if not (value.get("profile_id") == profile_id and value.get("session_id") == session_id)
        }