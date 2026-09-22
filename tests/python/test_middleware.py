"""Tests for Hermes middleware integration and privacy runtime."""

from __future__ import annotations

import pytest
from dataclasses import replace
from unittest.mock import AsyncMock, MagicMock

from gaze_privacy.middleware import PrivacyRuntime
from gaze_privacy.runtime import HermesCapabilities, StreamRegistry
from gaze_privacy.events import PrivacyEvent, EventBuffer
from gaze_privacy.provider_policy import ProviderPolicy, ProtectionDecision
from gaze_privacy.config import PrivacyConfig
from gaze_privacy.sidecar_manager import ManagedSidecar
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus


class FakeSidecarClient:
    def __init__(self):
        self.clean_calls = 0
        self.restore_calls = 0
        self.last_clean_namespace = None
        self.last_clean_fields = None

    async def clean(self, namespace, fields):
        self.clean_calls += 1
        self.last_clean_namespace = namespace
        self.last_clean_fields = fields
        cleaned = []
        for f in fields:
            path = f.path if hasattr(f, "path") else f["path"]
            text = f.text if hasattr(f, "text") else f["text"]
            from gaze_privacy.adapters.common import TextField
            cleaned.append(TextField(path=path, text=text.replace("alice@example.invalid", "<token>")))
        return {"fields": cleaned, "detections": [], "policy_version": "sha256:abc"}

    async def restore(self, namespace, fields):
        self.restore_calls += 1
        restored = []
        for f in fields:
            path = f.path if hasattr(f, "path") else f["path"]
            text = f.text if hasattr(f, "text") else f["text"]
            from gaze_privacy.adapters.common import TextField
            restored.append(TextField(path=path, text=text.replace("<token>", "alice@example.invalid")))
        return {"fields": restored}

    async def status(self):
        return SidecarStatus(protocol_version=1, gaze_version="0.14.0")


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
    rt = PrivacyRuntime(
        config=privacy_config,
        provider_policy=provider_policy,
        capabilities=capabilities,
        sidecars=fake_sidecar_manager,
        events=event_buffer,
    )
    rt.fake_sidecar = fake_sidecar_manager.client
    return rt


def fake_chat_response(content: str):
    return {
        "choices": [
            {
                "message": {
                    "content": content,
                }
            }
        ]
    }


def payload():
    return {"messages": [{"role": "user", "content": "Email alice@example.invalid"}]}


@pytest.mark.asyncio
async def test_external_provider_receives_only_cleaned_request(runtime):
    seen = {}

    def provider(request):
        seen["request"] = request
        return fake_chat_response("Hello <token>")

    result = await runtime.execute(
        request={"messages": [{"role": "user", "content": "Email alice@example.invalid"}]},
        next_call=provider,
        provider="openrouter",
        api_mode="chat_completions",
        session_id="s1",
        turn_id="t1",
        api_request_id="t1:api:1",
    )

    assert "alice@example.invalid" not in repr(seen["request"])
    assert result["choices"][0]["message"]["content"] == "Hello alice@example.invalid"


@pytest.mark.asyncio
async def test_fallback_rechecks_provider_trust(runtime):
    runtime.provider_policy = ProviderPolicy(frozenset({"local-vllm"}))
    calls = []

    await runtime.execute(
        provider="local-vllm",
        next_call=lambda request: calls.append(("local", request)) or fake_chat_response("ok"),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    await runtime.execute(
        provider="openrouter",
        next_call=lambda request: calls.append(("openrouter", request)) or fake_chat_response("ok"),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r2",
    )
    await runtime.execute(
        provider="anthropic",
        next_call=lambda request: calls.append(("anthropic", request)) or fake_chat_response("ok"),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r3",
    )

    assert runtime.fake_sidecar.clean_calls == 2
    assert [name for name, _ in calls] == ["local", "openrouter", "anthropic"]


@pytest.mark.asyncio
async def test_missing_hermes_privacy_capabilities_block_external(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.config = replace(runtime.config, mandatory_mode=True)
    with pytest.raises(Exception, match="PrivacyBlockedError|fail-closed"):
        await runtime.execute(
            provider="openrouter",
            next_call=lambda request: pytest.fail("provider must not be called"),
            request=payload(),
            api_mode="chat_completions",
            session_id="s1",
            api_request_id="r1",
        )


@pytest.mark.asyncio
async def test_missing_capabilities_still_allow_explicit_trusted_local(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.provider_policy = ProviderPolicy(frozenset({"local-vllm"}))
    before = runtime.fake_sidecar.clean_calls
    result = await runtime.execute(
        provider="local-vllm",
        next_call=lambda request: fake_chat_response("ok"),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    assert result is not None
    assert runtime.fake_sidecar.clean_calls == before


@pytest.mark.asyncio
async def test_compatibility_mode_marks_external_call_not_guaranteed(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.config = replace(runtime.config, compatibility_mode=True)
    await runtime.execute(
        provider="openrouter",
        next_call=lambda request: fake_chat_response("ok"),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    last_event = runtime.events.last()
    assert last_event is not None
    assert last_event["state"] == "protection_not_guaranteed"