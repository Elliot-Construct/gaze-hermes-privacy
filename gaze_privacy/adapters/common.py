"""Common types and utilities for provider adapters."""

from __future__ import annotations

import copy
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class TextField:
    path: str
    text: str


@dataclass
class PreparedPayload:
    payload: Any
    fields: list[TextField]

    def apply(self, transformed: list[TextField]) -> Any:
        if [f.path for f in transformed] != [f.path for f in self.fields]:
            raise PrivacyProtocolError("sidecar returned mismatched field paths")
        result = copy.deepcopy(self.payload)
        for field in transformed:
            set_path(result, field.path, field.text)
        return result


class UnsupportedCarrierError(Exception):
    """Raised when mandatory mode encounters uninspectable content (images, audio, unknown blobs)."""


class PrivacyProtocolError(Exception):
    """Raised when protocol invariants are violated."""


def get_path(obj: Any, path: str) -> Any:
    """Get value at JSON-pointer-like path (e.g., '/messages/0/content')."""
    parts = path.strip("/").split("/")
    current = obj
    for part in parts:
        if part.isdigit():
            current = current[int(part)]
        else:
            current = current[part]
    return current


def set_path(obj: Any, path: str, value: Any) -> None:
    """Set value at JSON-pointer-like path, creating intermediate dicts/lists as needed."""
    parts = path.strip("/").split("/")
    current = obj
    for i, part in enumerate(parts[:-1]):
        if part.isdigit():
            idx = int(part)
            if isinstance(current, list):
                current = current[idx]
            else:
                raise ValueError(f"Expected list at {path}, got {type(current)}")
        else:
            if part not in current:
                next_part = parts[i + 1]
                current[part] = [] if next_part.isdigit() else {}
            current = current[part]
    last = parts[-1]
    if last.isdigit():
        current[int(last)] = value
    else:
        current[last] = value