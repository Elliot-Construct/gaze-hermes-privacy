"""Host-scoped privacy plugin configuration."""

from __future__ import annotations

import os
import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class PrivacyConfig:
    home: Path
    sidecar_mode: str
    sidecar_scope: str
    sidecar_url: str
    trusted_local_providers: frozenset[str]
    mandatory_mode: bool
    compatibility_mode: bool
    api_token_file: Path
    master_key_file: Path
    global_policy_file: Path
    profile_policy_dir: Path

    @classmethod
    def load(cls) -> "PrivacyConfig":
        override = os.getenv("GAZE_HERMES_HOME", "").strip()
        if override:
            home = Path(override).expanduser()
        else:
            from hermes_constants import get_default_hermes_root

            home = Path(get_default_hermes_root()) / "gaze-hermes-privacy"
        raw = (
            tomllib.loads((home / "config.toml").read_text("utf-8"))
            if (home / "config.toml").exists()
            else {}
        )
        sidecar = raw.get("sidecar", {})
        providers = raw.get("providers", {})
        security = raw.get("security", {})
        scope = str(sidecar.get("scope", "host"))
        if scope not in {"host", "profile"}:
            raise ValueError("sidecar.scope must be 'host' or 'profile'")
        mode = str(sidecar.get("mode", "native"))
        if mode not in {"native", "docker", "external"}:
            raise ValueError("sidecar.mode must be native, docker, or external")
        return cls(
            home=home,
            sidecar_mode=mode,
            sidecar_scope=scope,
            sidecar_url=str(sidecar.get("url", "http://127.0.0.1:65113")),
            trusted_local_providers=frozenset(map(str, providers.get("trusted_local", []))),
            mandatory_mode=bool(security.get("mandatory", True)),
            compatibility_mode=bool(security.get("compatibility_mode", False)),
            api_token_file=Path(sidecar.get("api_token_file", home / "secrets" / "api-token")),
            master_key_file=Path(sidecar.get("master_key_file", home / "secrets" / "snapshot-key")),
            global_policy_file=Path(
                raw.get("policy", {}).get("global_file", home / "policies" / "global.toml")
            ),
            profile_policy_dir=Path(
                raw.get("policy", {}).get("profile_dir", home / "policies" / "profiles")
            ),
        )
