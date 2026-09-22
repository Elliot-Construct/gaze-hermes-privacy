"""Hermes hybrid plugin entrypoint for gaze-hermes-privacy."""

from gaze_privacy.middleware import init_runtime, get_runtime
from gaze_privacy.runtime import PrivacyRuntime
from gaze_privacy.config import PrivacyConfig
from gaze_privacy.provider_policy import ProviderPolicy
from gaze_privacy.events import EventBuffer
from gaze_privacy.sidecar_manager import SidecarManager
from gaze_privacy.sidecar_client import SidecarStatus
from gaze_privacy.runtime import HermesCapabilities

import inspect


def detect_hermes_capabilities(ctx) -> HermesCapabilities:
    """Detect Hermes middleware capabilities at plugin registration time."""
    fail_closed = False
    stream_text = False

    try:
        params = inspect.signature(ctx.register_middleware).parameters
        fail_closed = "failure_mode" in params
    except (TypeError, ValueError):
        fail_closed = False

    try:
        from hermes_cli.middleware import VALID_MIDDLEWARE
        stream_text = "llm_stream_text" in VALID_MIDDLEWARE
    except Exception:
        stream_text = False

    return HermesCapabilities(fail_closed=fail_closed, stream_text=stream_text)


def register(ctx):
    """Register the gaze-hermes-privacy plugin with Hermes."""
    config = PrivacyConfig.load()

    provider_policy = ProviderPolicy(config.trusted_local_providers)

    # Detect Hermes capabilities at registration time
    capabilities = detect_hermes_capabilities(ctx)
    events = EventBuffer()
    sidecars = SidecarManager(config)

    runtime = init_runtime(config, provider_policy, capabilities, sidecars, events)

    # Store runtime in plugin context for middleware access
    ctx.runtime = runtime

    # Register execution middleware
    if capabilities.fail_closed:
        ctx.register_middleware(
            "llm_execution",
            llm_execution_middleware,
            failure_mode="closed",
        )
    else:
        # Compatibility mode: register without failure_mode
        ctx.register_middleware("llm_execution", llm_execution_middleware)

    # Register streaming middleware if supported
    if capabilities.stream_text:
        kwargs = {"failure_mode": "closed"} if capabilities.fail_closed else {}
        ctx.register_middleware("llm_stream_text", llm_stream_text_middleware, **kwargs)

    # Register backend API for Desktop using Hermes' supported mechanism
    # Note: ctx.register_api is not a standard Hermes API; use ctx.register_backend_api or similar if available
    # For now, we'll attach the API router to the runtime for middleware access
    runtime.plugin_api_service = None  # Will be set by init_runtime via PluginApiService.from_runtime


def llm_execution_middleware(*, request, next_call, provider, api_mode, **ctx):
    """llm_execution middleware - synchronous entry point."""
    runtime = get_runtime()
    if runtime is None:
        raise RuntimeError("PrivacyRuntime not initialized. Plugin may not be properly registered.")
    return runtime.execute_sync(
        request=request,
        next_call=next_call,
        provider=provider,
        api_mode=api_mode,
        **ctx
    )


def llm_stream_text_middleware(*, text, kind, provider, session_id, api_request_id, **context):
    """llm_stream_text middleware - synchronous entry point."""
    runtime = get_runtime()
    if runtime is None:
        raise RuntimeError("PrivacyRuntime not initialized. Plugin may not be properly registered.")
    return runtime.stream_text_sync(
        text=text,
        kind=kind,
        provider=provider,
        session_id=session_id,
        api_request_id=api_request_id,
    )


def ws_upgrade_authorized(websocket):
    """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
    try:
        from hermes_cli import web_server_chat as _ws
    except Exception:
        return True
    return bool(_ws._ws_auth_ok(websocket))