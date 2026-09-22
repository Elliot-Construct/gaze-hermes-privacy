"""Native-first sidecar lifecycle management."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from gaze_privacy.config import PrivacyConfig
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus
from gaze_privacy.secrets import ensure_secret, ensure_secret_bytes


SIDECAR_PROTOCOL_VERSION = 1


@dataclass(frozen=True)
class ManagedSidecar:
    status: SidecarStatus
    client: SidecarClient


class SidecarManager:
    """Manages the native sidecar process lifecycle with scope awareness."""

    def __init__(self, config: PrivacyConfig, launcher: Any = None):
        self.config = config
        self._launcher = launcher
        self._host_endpoint: str | None = None
        self._host_client: SidecarClient | None = None
        self._profile_processes: dict[str, subprocess.Popen] = {}
        self._profile_endpoints: dict[str, str] = {}
        self._scope_locks: dict[str, threading.Lock] = {}
        self._global_lock = threading.Lock()

    def _get_lock(self, scope: str) -> threading.Lock:
        with self._global_lock:
            if scope not in self._scope_locks:
                self._scope_locks[scope] = threading.Lock()
            return self._scope_locks[scope]

    def _api_token(self) -> str:
        return ensure_secret(self.config.api_token_file).strip()

    def _master_key(self) -> bytes:
        return ensure_secret_bytes(self.config.master_key_file)

    def _sidecar_binary(self) -> Path:
        # In native mode, the binary is expected at a known location
        # For development, it's in the sidecar/target/release directory
        # For installed plugins, it would be in the plugin's bin directory
        candidate = Path(__file__).parent.parent.parent / "sidecar" / "target" / "release" / "gaze-hermes-sidecar"
        if candidate.exists():
            return candidate
        # Fallback: assume it's in PATH
        return Path("gaze-hermes-sidecar")

    def _verify_binary(self, binary_path: Path) -> bool:
        """Verify binary SHA-256 against release manifest."""
        # TODO: Load and check against sidecar-release.json
        # For now, just check it exists and is executable
        return binary_path.exists() and os.access(binary_path, os.X_OK)

    async def _wait_ready(self, client: SidecarClient, timeout: float = 10.0) -> SidecarStatus:
        start = time.monotonic()
        while time.monotonic() - start < timeout:
            try:
                status = await client.status()
                if status.protocol_version == SIDECAR_PROTOCOL_VERSION:
                    return status
            except Exception:
                pass
            await asyncio.sleep(0.2)
        raise RuntimeError("Sidecar failed to become ready")

    def _launch_host_sidecar(self) -> tuple[str, subprocess.Popen]:
        """Launch host-scoped sidecar on fixed port."""
        binary = self._sidecar_binary()
        if not self._verify_binary(binary):
            raise RuntimeError(f"Sidecar binary not found or invalid: {binary}")

        env = os.environ.copy()
        env["GAZE_SIDECAR_API_TOKEN_FILE"] = str(self.config.api_token_file)
        env["GAZE_SIDECAR_MASTER_KEY_FILE"] = str(self.config.master_key_file)
        env["GAZE_SIDECAR_DATA_DIR"] = str(self.config.home / "data")
        env["GAZE_SIDECAR_BIND"] = "127.0.0.1:65113"

        proc = subprocess.Popen(
            [str(binary)],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

        endpoint = "http://127.0.0.1:65113"
        return endpoint, proc

    def _launch_profile_sidecar(self, profile_id: str) -> tuple[str, subprocess.Popen]:
        """Launch profile-scoped sidecar with ephemeral port and ready file."""
        binary = self._sidecar_binary()
        if not self._verify_binary(binary):
            raise RuntimeError(f"Sidecar binary not found or invalid: {binary}")

        # Create profile-specific run directory
        run_dir = self.config.home / "run" / profile_id
        run_dir.mkdir(parents=True, exist_ok=True)
        ready_file = run_dir / "ready.json"

        # Profile-specific data directory for snapshot isolation
        profile_data_dir = self.config.home / "data" / profile_id
        profile_data_dir.mkdir(parents=True, exist_ok=True)

        env = os.environ.copy()
        env["GAZE_SIDECAR_API_TOKEN_FILE"] = str(self.config.api_token_file)
        env["GAZE_SIDECAR_MASTER_KEY_FILE"] = str(self.config.master_key_file)
        env["GAZE_SIDECAR_DATA_DIR"] = str(profile_data_dir)
        env["GAZE_SIDECAR_BIND"] = "127.0.0.1:0"
        env["GAZE_SIDECAR_READY_FILE"] = str(ready_file)

        proc = subprocess.Popen(
            [str(binary)],
            env=env,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=run_dir,
        )

        # Wait for ready file to appear with port info
        for _ in range(50):  # 5 second timeout
            if ready_file.exists():
                try:
                    data = json.loads(ready_file.read_text(encoding="utf-8"))
                    # Support both old format (port) and new format (address)
                    if "port" in data:
                        port = data["port"]
                    elif "address" in data:
                        addr = data["address"]
                        if isinstance(addr, str) and ":" in addr:
                            port = int(addr.split(":")[-1])
                        else:
                            continue
                    else:
                        continue
                    endpoint = f"http://127.0.0.1:{port}"
                    return endpoint, proc
                except Exception:
                    pass
            time.sleep(0.1)

        proc.terminate()
        raise RuntimeError("Profile sidecar failed to produce ready file")

    async def ensure_running(self, profile_id: str) -> ManagedSidecar:
        """Ensure a sidecar is running for the given profile."""
        scope = "host" if self.config.sidecar_scope == "host" else profile_id
        lock = self._get_lock(scope)

        with lock:
            # Check for existing healthy endpoint
            endpoint = await self._existing_healthy_endpoint(scope)
            if endpoint is None:
                endpoint = await self._launch_or_resolve_endpoint(scope, profile_id)

            client = SidecarClient(endpoint, self._api_token())
            status = await client.status()

            if status.protocol_version != SIDECAR_PROTOCOL_VERSION:
                raise RuntimeError(f"Sidecar protocol mismatch: expected {SIDECAR_PROTOCOL_VERSION}, got {status.protocol_version}")

            # Cache for host scope
            if scope == "host":
                self._host_endpoint = endpoint
                self._host_client = client

            return ManagedSidecar(status=status, client=client)

    async def _existing_healthy_endpoint(self, scope: str) -> str | None:
        """Check if we have a cached healthy endpoint for this scope."""
        if scope == "host" and self._host_endpoint and self._host_client:
            try:
                status = await self._host_client.status()
                if status.protocol_version == SIDECAR_PROTOCOL_VERSION:
                    return self._host_endpoint
            except Exception:
                pass
        elif scope != "host" and scope in self._profile_endpoints:
            endpoint = self._profile_endpoints[scope]
            try:
                client = SidecarClient(endpoint, self._api_token())
                status = await client.status()
                if status.protocol_version == SIDECAR_PROTOCOL_VERSION:
                    return endpoint
            except Exception:
                pass
        return None

    async def _launch_or_resolve_endpoint(self, scope: str, profile_id: str) -> str:
        """Launch a new sidecar or resolve endpoint based on mode."""
        if self.config.sidecar_mode == "external":
            return self.config.sidecar_url

        if self.config.sidecar_mode == "docker":
            return self.config.sidecar_url

        # Native mode
        if scope == "host":
            endpoint, proc = self._launch_host_sidecar()
        else:
            endpoint, proc = self._launch_profile_sidecar(profile_id)
            self._profile_processes[scope] = proc
            self._profile_endpoints[scope] = endpoint

        # Wait for readiness
        client = SidecarClient(endpoint, self._api_token())
        await self._wait_ready(client)

        return endpoint