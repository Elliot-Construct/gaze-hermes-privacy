"""Release manifest handling for sidecar artifacts."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Artifact:
    os: str
    arch: str
    url: str
    sha256: str


@dataclass(frozen=True)
class ReleaseManifest:
    protocol_version: int
    artifacts: tuple[Artifact, ...]

    @classmethod
    def load(cls, path: Path) -> "ReleaseManifest":
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("protocol_version") != 1:
            raise ManifestError(f"Unsupported protocol version: {data.get('protocol_version')}")

        artifacts = []
        seen = set()
        for a in data.get("artifacts", []):
            if not all(k in a for k in ("os", "arch", "url", "sha256")):
                raise ManifestError("Artifact missing required fields")
            if len(a["sha256"]) != 64:
                raise ManifestError(f"Invalid SHA-256 length: {a['sha256']}")
            try:
                bytes.fromhex(a["sha256"])
            except ValueError:
                raise ManifestError(f"Invalid SHA-256 hex: {a['sha256']}")

            key = (a["os"], a["arch"])
            if key in seen:
                raise ManifestError(f"Duplicate artifact target: {key}")
            seen.add(key)

            if not a["url"].startswith("https://") and not a["url"].startswith("file://"):
                raise ManifestError(f"Non-HTTPS URL not allowed: {a['url']}")

            artifacts.append(Artifact(os=a["os"], arch=a["arch"], url=a["url"], sha256=a["sha256"]))

        return cls(protocol_version=data["protocol_version"], artifacts=tuple(artifacts))

    def find_artifact(self, os: str, arch: str) -> Artifact | None:
        for a in self.artifacts:
            if a.os == os and a.arch == arch:
                return a
        return None


class ManifestError(Exception):
    """Raised when manifest validation fails."""