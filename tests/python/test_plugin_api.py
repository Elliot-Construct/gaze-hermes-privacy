"""Tests for Desktop plugin API service."""

from __future__ import annotations

import json
import pytest
from dataclasses import replace
from unittest.mock import AsyncMock, MagicMock

from fastapi.testclient import TestClient

from gaze_privacy.plugin_api_service import PluginApiService
from gaze_privacy.reveal import RevealService
from gaze_privacy.runtime import HermesCapabilities, PrivacyRuntime
from gaze_privacy.events import EventBuffer
from gaze_privacy.provider_policy import ProviderPolicy, ProtectionDecision
from gaze_privacy.config import PrivacyConfig
from gaze_privacy.sidecar_manager import SidecarManager, ManagedSidecar
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus
from gaze_privacy.adapters import PreparedPayload, TextField


class FakeSidecarClient:
    def __init__(self):
        self.clean_calls = 0

    async def clean(self, namespace, fields):
        self.clean_calls += 1
        cleaned = []
        for f in fields:
            cleaned.append({"path": f["path"], "text": f["text"].replace("alice@example.invalid", "<token>")})
        return {"fields": cleaned, "detections": [], "policy_version": "sha256:abc"}

    async def restore(self, namespace, fields):
        restored = []
        for f in fields:
            restored.append({"path": f["path"], "text": f["text"].replace("<token>", "alice@example.invalid")})
        return {"fields": restored}

    async def status(self):
        return SidecarStatus(protocol_version=1, gaze_version="0.14.0", ner_model="kiji-distilbert")

    async def validate_policy(self, toml):
        return {"valid": True, "errors": []}

    async def edit_policy(self, scope, edit):
        return {"toml": "edited", "base_hash": "hash123"}

    async def apply_policy(self, scope, expected_hash, toml):
        return {"toml": toml, "policy_hash": "newhash"}

    async def effective_policy(self, profile_id):
        return {"toml": "policy", "policy_hash": "hash123"}

    async def list_sessions(self):
        return {"sessions": []}

    async def get_session(self, profile_id, session_id):
        return {"profile_id": profile_id, "session_id": session_id, "snapshot_state": "valid", "mapping_count": 0}

    async def recover_session(self, profile_id, session_id):
        return {"recovered": True}

    async def delete_session(self, profile_id, session_id):
        pass

    async def metrics(self):
        return {"counters": {"protected": 0, "bypassed": 0, "blocked": 0}}


class FakeSidecarManager:
    def __init__(self, client=None):
        self.client = client or FakeSidecarClient()

    async def ensure_running(self, profile_id):
        return ManagedSidecar(
            status=SidecarStatus(protocol_version=1, gaze_version="0.14.0"),
            client=self.client,
        )


@pytest.fixture
def fake_sidecar_client():
    return FakeSidecarClient()


@pytest.fixture
def fake_sidecar_manager(fake_sidecar_client):
    return FakeSidecarManager(fake_sidecar_client)


@pytest.fixture
def privacy_config(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    (tmp_path / "secrets").mkdir(exist_ok=True)
    (tmp_path / "secrets" / "api-token").write_text("test-token", encoding="utf-8")
    (tmp_path / "secrets" / "snapshot-key").write_text("x" * 43, encoding="utf-8")
    return PrivacyConfig.load()


@pytest.fixture
def provider_policy():
    return ProviderPolicy(frozenset())


@pytest.fixture
def capabilities():
    return HermesCapabilities(fail_closed=True, stream_text=True)


@pytest.fixture
def event_buffer():
    return EventBuffer(max_events=100)


@pytest.fixture
def runtime(privacy_config, provider_policy, capabilities, fake_sidecar_manager, event_buffer):
    return PrivacyRuntime(
        config=privacy_config,
        provider_policy=provider_policy,
        capabilities=capabilities,
        sidecars=fake_sidecar_manager,
        events=event_buffer,
    )


@pytest.fixture
def reveal_service():
    return RevealService()

@pytest.fixture
def plugin_service(runtime, fake_sidecar_manager, event_buffer, reveal_service):
    return PluginApiService(runtime, fake_sidecar_manager, event_buffer, reveal_service)


from fastapi import FastAPI

@pytest.fixture
def app(plugin_service):
    app = FastAPI()
    app.include_router(plugin_service.router)
    return app

@pytest.fixture
def client(app):
    return TestClient(app)


def test_status_is_desktop_safe(client):
    response = client.get("/status")
    assert response.status_code == 200
    body = response.json()
    assert body["sidecar"]["mode"] in {"native", "docker", "external"}
    serialized = json.dumps(body)
    assert "api_token" not in serialized
    assert "master_key" not in serialized
    assert "Authorization" not in serialized


def test_invalid_policy_never_replaces_active_policy(client):
    # First get the active policy
    active_response = client.get("/policies/global")
    assert active_response.status_code == 200
    active_policy_text = active_response.json()["toml"]

    # Try to apply invalid policy
    response = client.post("/policies/apply", json={"scope": "global", "toml": "not = [valid"})
    assert response.status_code == 422

    # Active policy should be unchanged
    get_response = client.get("/policies/global")
    assert get_response.status_code == 200
    assert get_response.json()["toml"] == active_policy_text