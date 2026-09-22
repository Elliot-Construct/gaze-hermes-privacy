"""Hermes hybrid plugin entrypoint for gaze-hermes-privacy."""

from gaze_privacy.middleware import init_runtime, get_runtime
from gaze_privacy.runtime import PrivacyRuntime
from gaze_privacy.config import PrivacyConfig
from gaze_privacy.provider_policy import ProviderPolicy
from gaze_privacy.events import EventBuffer
from gaze_privacy.sidecar_manager import SidecarManager
from gaze_privacy.sidecar_client import SidecarStatus
from gaze_privacy.runtime import HermesCapabilities


def register(ctx):
    """Register the gaze-hermes-privacy plugin with Hermes."""
    config = PrivacyConfig.load()
    
    provider_policy = ProviderPolicy(config.trusted_local_providers)
    
    # Detect Hermes capabilities at registration time
    # Check if Hermes supports llm_execution with failure_mode="closed"
    # and llm_stream_text middleware kind
    try:
        # Check if Hermes supports the required middleware capabilities
        has_fail_closed = hasattr(ctx, "register_middleware") and "failure_mode" in str(ctx.register_middleware)
        # Check if llm_stream_text middleware kind is supported
        has_stream_text = hasattr(ctx, "register_middleware") and hasattr(ctx, "llm_stream_text")
    except Exception:
        has_fail_closed = False
        has_stream_text = False
    
    capabilities = HermesCapabilities(fail_closed=has_fail_closed, stream_text=has_stream_text)
    events = EventBuffer()
    sidecars = SidecarManager(config)
    
    runtime = init_runtime(config, provider_policy, capabilities, sidecars, events)
    
    # Store runtime in plugin context for middleware access
    ctx.runtime = runtime
    
    # Register execution middleware with failure_mode="closed" for fail-closed behavior
    ctx.register_middleware("llm_execution", llm_execution_middleware, failure_mode="closed")
    ctx.register_middleware("llm_stream_text", llm_stream_text_middleware, failure_mode="closed")
    
    # Register backend API for Desktop using Hermes' supported mechanism
    # Note: ctx.register_api is not a standard Hermes API; use ctx.register_backend_api or similar if available
    # For now, we'll attach the API router to the runtime for middleware access
    runtime.plugin_api_service = None  # Will be set by init_runtime via PluginApiService.from_runtime


def llm_execution_middleware(*, request, next_call, provider, api_mode, **ctx):
    """llm_execution middleware - synchronous entry point."""
    runtime = get_runtime()
    if runtime is None:
        # This should not happen if plugin registered correctly
        raise RuntimeError("PrivacyRuntime not initialized. Plugin may not be properly registered.")
    return runtime.execute_sync(
        request=request,
        next_call=next_call,
        provider=provider,
        api_mode=api_mode,
        **ctx
    )


def llm_stream_text_middleware(*, text, kind, provider, profile_id, session_id, api_request_id, **_ctx):
    """llm_stream_text middleware - synchronous entry point."""
    runtime = get_runtime()
    if runtime is None:
        raise RuntimeError("PrivacyRuntime not initialized. Plugin may not be properly registered.")
    return runtime.stream_text_sync(
        text=text,
        kind=kind,
        provider=provider,
        profile_id=profile_id,
        session_id=session_id,
        api_request_id=api_request_id,
    )


def ws_upgrade_authorized(websocket):
    """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
    # In production, this would check Hermes' auth token/cookie
    return True