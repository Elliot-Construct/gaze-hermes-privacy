"""Sanitised event buffer for privacy audit logging."""

from __future__ import annotations

import time
from collections import deque
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class PrivacyEvent:
    id: str
    timestamp: float
    provider: str
    api_mode: str
    session_id: str
    profile_id: str
    request_id: str
    state: str
    detections: list[dict[str, Any]] = field(default_factory=list)
    latency_ms: float | None = None
    error_code: str | None = None

    def sanitised_dict(self) -> dict[str, Any]:
        return {
            "id": self.id,
            "timestamp": self.timestamp,
            "provider": self.provider,
            "api_mode": self.api_mode,
            "session_id": self.session_id,
            "profile_id": self.profile_id,
            "request_id": self.request_id,
            "state": self.state,
            "detections": self.detections,
            "latency_ms": self.latency_ms,
            "error_code": self.error_code,
        }


class EventBuffer:
    """Bounded event buffer with optional sensitive payload capture."""

    def __init__(self, max_events: int = 500):
        self._events = deque(maxlen=max_events)
        self._sensitive: dict[str, dict[str, str]] = {}
        self._counter = 0

    def add(self, event: PrivacyEvent, sensitive: dict[str, str] | None = None) -> str:
        self._events.append(event.sanitised_dict())
        if sensitive is not None:
            self._sensitive[event.id] = dict(sensitive)
        return event.id

    def add_sanitised(self, event: PrivacyEvent) -> str:
        return self.add(event, sensitive=None)

    def list(self, limit: int = 100) -> list[dict[str, Any]]:
        return list(self._events)[-limit:]

    def last(self) -> dict[str, Any] | None:
        if self._events:
            return self._events[-1]
        return None

    def clear_session(self, profile_id: str, session_id: str) -> None:
        self._sensitive = {
            event_id: value
            for event_id, value in self._sensitive.items()
            if not (value.get("profile_id") == profile_id and value.get("session_id") == session_id)
        }

    def serialized_log(self) -> str:
        import json
        return json.dumps(list(self._events))


def generate_event_id() -> str:
    import uuid
    return uuid.uuid4().hex[:16]


def create_event(
    provider: str,
    api_mode: str,
    session_id: str,
    profile_id: str,
    request_id: str,
    state: str,
    detections: list[dict[str, Any]] | None = None,
    latency_ms: float | None = None,
    error_code: str | None = None,
) -> PrivacyEvent:
    return PrivacyEvent(
        id=generate_event_id(),
        timestamp=time.time(),
        provider=provider,
        api_mode=api_mode,
        session_id=session_id,
        profile_id=profile_id,
        request_id=request_id,
        state=state,
        detections=detections or [],
        latency_ms=latency_ms,
        error_code=error_code,
    )