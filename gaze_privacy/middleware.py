"""Hermes middleware callbacks bound to PrivacyRuntime."""

from __future__ import annotations

from typing import Any, Callable

from gaze_privacy.runtime import PrivacyRuntime, HermesCapabilities, StreamRegistry
from gaze_privacy.events import EventBuffer
from gaze_privacy.provider_policy import ProviderPolicy, ProtectionDecision
from gaze_privacy.config import PrivacyConfig
from gaze_privacy.sidecar_manager import SidecarManager
from gaze_privacy.plugin_api_service import PluginApiService


_runtime: PrivacyRuntime | None = None


def init_runtime(
    config: PrivacyConfig,
    provider_policy: ProviderPolicy,
    capabilities: HermesCapabilities,
    sidecars: SidecarManager,
    events: EventBuffer,
) -> PrivacyRuntime:
    """Initialize the global privacy runtime."""
    global _runtime
    _runtime = PrivacyRuntime(
        config=config,
        provider_policy=provider_policy,
        capabilities=capabilities,
        sidecars=sidecars,
        events=events,
    )
    # Initialize plugin API service
    _runtime.plugin_api_service = PluginApiService(_runtime)
    return _runtime


def get_runtime() -> PrivacyRuntime:
    if _runtime is None:
        raise RuntimeError("PrivacyRuntime not initialized. Call init_runtime() first.")
    return _runtime


def llm_execution_middleware(
    *,
    request: dict[str, Any],
    next_call: Callable[[dict[str, Any]], Any],
    provider: str,
    api_mode: str,
    **context: Any,
) -> Any:
    """llm_execution middleware callback for Hermes (synchronous)."""
    return get_runtime().execute_sync(
        request=request,
        next_call=next_call,
        provider=provider,
        api_mode=api_mode,
        **context,
    )


def llm_stream_text_middleware(
    *,
    text: str,
    kind: str,
    provider: str,
    profile_id: str,
    session_id: str,
    api_request_id: str,
    **context: Any,
) -> dict[str, str]:
    """llm_stream_text middleware callback for Hermes (synchronous)."""
    runtime = get_runtime()

    if runtime.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
        return {"text": text}

    runtime.require_or_mark_capabilities(provider=provider, context=context)

    key = (
        str(profile_id or "default"),
        str(session_id or ""),
        str(api_request_id or ""),
    )
    stream = runtime.streams.get_or_open(key)
    restored = stream.feed_sync(kind=kind, text=text)
    return {"text": restored}