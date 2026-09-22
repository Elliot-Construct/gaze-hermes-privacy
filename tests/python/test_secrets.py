"""Tests for secret generation and management."""

from __future__ import annotations

import os
import stat
from pathlib import Path

import pytest

from gaze_privacy.secrets import ensure_secret


def test_generated_secret_is_persistent_and_private(tmp_path):
    path = tmp_path / "api-token"
    first = ensure_secret(path)
    second = ensure_secret(path)
    assert first == second
    assert first
    if os.name != "nt":
        assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_existing_secret_never_overwritten(tmp_path):
    path = tmp_path / "api-token"
    path.write_text("operator-supplied", encoding="utf-8")
    result = ensure_secret(path)
    assert result == "operator-supplied"
    assert path.read_text(encoding="utf-8") == "operator-supplied"


def test_master_key_generation_uses_random_bytes(tmp_path):
    path = tmp_path / "snapshot-key"
    key = ensure_secret(path, nbytes=32)
    assert len(key) > 0
    # base64url encoding of 32 bytes = 43 chars (no padding)
    # secrets.token_urlsafe(32) produces 43 chars
    assert len(key) >= 43


def test_ensure_secret_creates_parent_directories(tmp_path):
    path = tmp_path / "deep" / "nested" / "secret"
    result = ensure_secret(path)
    assert result
    assert path.exists()


def test_different_nbytes_produces_different_lengths(tmp_path):
    path1 = tmp_path / "secret-16"
    path2 = tmp_path / "secret-32"
    key1 = ensure_secret(path1, nbytes=16)
    key2 = ensure_secret(path2, nbytes=32)
    # token_urlsafe: 16 bytes -> 22 chars, 32 bytes -> 43 chars
    assert len(key2) > len(key1)