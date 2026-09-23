"""Secret generation and management with POSIX permissions."""

from __future__ import annotations

import os
import secrets
from pathlib import Path


def ensure_secret(path: Path, *, nbytes: int = 32) -> str:
    """Ensure a secret file exists, generating it if necessary.

    Args:
        path: Path to the secret file.
        nbytes: Number of random bytes to generate (default 32).

    Returns:
        The secret value (existing or newly generated) as URL-safe text.
    """
    if path.exists():
        return path.read_text(encoding="utf-8").strip()

    path.parent.mkdir(parents=True, exist_ok=True)
    value = secrets.token_urlsafe(nbytes)

    # Use os.open with O_EXCL to prevent race conditions; set mode to 0o600
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    mode = 0o600 if os.name != "nt" else 0o600
    fd = os.open(path, flags, mode)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(value)
    except Exception:
        # Clean up on failure
        try:
            os.unlink(path)
        except OSError:
            pass
        raise

    return value


def ensure_secret_bytes(path: Path, *, nbytes: int = 32) -> bytes:
    """Ensure a secret file exists with raw bytes (not URL-safe encoded).

    Used for master keys that must be exactly 32 raw bytes for Rust sidecar.

    Args:
        path: Path to the secret file.
        nbytes: Number of random bytes to generate (default 32).

    Returns:
        The secret value (existing or newly generated) as raw bytes.
    """
    if path.exists():
        return path.read_bytes()

    path.parent.mkdir(parents=True, exist_ok=True)
    value = secrets.token_bytes(nbytes)

    # Use os.open with O_EXCL to prevent race conditions; set mode to 0o600
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL
    mode = 0o600 if os.name != "nt" else 0o600
    fd = os.open(path, flags, mode)
    try:
        with os.fdopen(fd, "wb") as handle:
            handle.write(value)
    except Exception:
        # Clean up on failure
        try:
            os.unlink(path)
        except OSError:
            pass
        raise

    return value