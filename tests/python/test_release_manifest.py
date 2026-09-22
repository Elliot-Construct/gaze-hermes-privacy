"""Tests for release manifest validation."""

from __future__ import annotations

import json
import pytest
from pathlib import Path

from gaze_privacy.release_manifest import ReleaseManifest, ManifestError, Artifact


def test_release_manifest_rejects_bad_digest(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            "sha256": "not-a-digest",
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="SHA-256 length"):
        ReleaseManifest.load(path)


def test_release_manifest_rejects_duplicate_target(tmp_path):
    artifact = {
        "os": "linux",
        "arch": "x86_64",
        "url": "https://example.invalid/sidecar",
        "sha256": "0" * 64,
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps({"protocol_version": 1, "artifacts": [artifact, artifact]}), encoding="utf-8")
    with pytest.raises(ManifestError, match="Duplicate artifact target"):
        ReleaseManifest.load(path)


def test_release_manifest_rejects_missing_fields(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            # missing sha256
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="required fields"):
        ReleaseManifest.load(path)


def test_release_manifest_rejects_invalid_sha256_length(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            "sha256": "abc",  # too short
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="SHA-256"):
        ReleaseManifest.load(path)


def test_release_manifest_rejects_non_hex_sha256(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            "sha256": "g" * 64,  # invalid hex
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="SHA-256"):
        ReleaseManifest.load(path)


def test_release_manifest_rejects_non_https_url(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "http://example.invalid/sidecar",  # not https
            "sha256": "0" * 64,
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="HTTPS"):
        ReleaseManifest.load(path)


def test_release_manifest_accepts_valid_manifest(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            "sha256": "0" * 64,
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    manifest_obj = ReleaseManifest.load(path)
    assert manifest_obj.protocol_version == 1
    assert len(manifest_obj.artifacts) == 1
    assert manifest_obj.artifacts[0].os == "linux"
    assert manifest_obj.artifacts[0].arch == "x86_64"


def test_find_artifact():
    manifest = ReleaseManifest(
        protocol_version=1,
        artifacts=(
            Artifact(os="linux", arch="x86_64", url="https://example.invalid/a", sha256="0" * 64),
            Artifact(os="darwin", arch="arm64", url="https://example.invalid/b", sha256="1" * 64),
        )
    )
    assert manifest.find_artifact("linux", "x86_64") is not None
    assert manifest.find_artifact("darwin", "arm64") is not None
    assert manifest.find_artifact("windows", "x86_64") is None