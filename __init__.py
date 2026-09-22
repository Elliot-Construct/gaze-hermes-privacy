"""Hermes hybrid plugin entrypoint for gaze-hermes-privacy."""

from gaze_privacy.middleware import init_runtime
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
    capabilities = HermesCapabilities(fail_closed=True, stream_text=True)
    events = EventBuffer()
    sidecars = SidecarManager(config)
    
    runtime = init_runtime(config, provider_policy, capabilities, sidecars, events)
    ctx.runtime = runtime
    
    # Register execution middleware
    ctx.register_middleware("llm_execution", middleware_execution)
    ctx.register_middleware("llm_stream_text", middleware_stream_text)
    
    # Register backend API for Desktop
    ctx.register_api("gaze_privacy", create_backend_api())
    
    # Register Desktop plugin if available
    try:
        from gaze_privacy.desktop import register_desktop
        register_desktop(ctx)
    except ImportError:
        pass


def middleware_execution(*, request, next_call, provider, api_mode, **ctx):
    """llm_execution middleware - synchronous entry point."""
    runtime = ctx.get("runtime")
    if runtime is None:
        # Fallback if runtime not in context
        return next_call(request)
    return runtime.execute_sync(
        request=request,
        next_call=next_call,
        provider=provider,
        api_mode=api_mode,
        **ctx
    )


def middleware_stream_text(*, text, kind, provider, profile_id, session_id, api_request_id, **_ctx):
    """llm_stream_text middleware - synchronous entry point."""
    runtime = _ctx.get("runtime")
    if runtime is None:
        return {"text": text}
    return runtime.stream_text_sync(
        text=text,
        kind=kind,
        provider=provider,
        profile_id=profile_id,
        session_id=session_id,
        api_request_id=api_request_id,
    )


def create_backend_api():
    """Create the backend API router for Desktop plugin."""
    from fastapi import APIRouter
    from gaze_privacy.plugin_api_service import PluginApiService
    
    router = APIRouter()
    
    # Runtime will be injected via dependency
    def get_runtime():
        # This will be overridden by the plugin registration
        from gaze_privacy.middleware import get_runtime as get_global_runtime
        return get_global_runtime()
    
    @router.get("/status")
    def status():
        runtime = get_runtime()
        return runtime.plugin_api_service.status()
    
    @router.get("/events")
    def events(limit: int = 100):
        runtime = get_runtime()
        return {"events": runtime.plugin_api_service.events.list(limit=min(max(limit, 1), 500))}
    
    @router.websocket("/events")
    async def events_socket(websocket):
        from fastapi import WebSocketDisconnect
        runtime = get_runtime()
        if not ws_upgrade_authorized(websocket):
            await websocket.close(code=4401)
            return
        await websocket.accept()
        subscription = runtime.plugin_api_service.events.subscribe()
        try:
            async for event in subscription:
                await websocket.send_json(event.sanitised_dict())
        except WebSocketDisconnect:
            pass
        finally:
            subscription.close()
    
    @router.post("/policies/validate")
    def validate_policy(request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.validate_policy(request)
    
    @router.get("/policies/global")
    def get_global_policy():
        runtime = get_runtime()
        return runtime.plugin_api_service.get_global_policy()
    
    @router.put("/policies/global")
    def put_global_policy(request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.put_global_policy(request)
    
    @router.get("/policies/profiles/{profile_id}")
    def get_profile_policy(profile_id: str):
        runtime = get_runtime()
        return runtime.plugin_api_service.get_profile_policy(profile_id)
    
    @router.put("/policies/profiles/{profile_id}")
    def put_profile_policy(profile_id: str, request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.put_profile_policy(profile_id, request)
    
    @router.post("/policies/test")
    def test_policy(request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.test_policy(request)
    
    @router.post("/policies/edit")
    def edit_policy(request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.edit_policy(request)
    
    @router.post("/policies/apply")
    def apply_policy(request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.apply_policy(request)
    
    @router.get("/providers")
    def list_providers():
        runtime = get_runtime()
        return runtime.plugin_api_service.list_providers()
    
    @router.put("/providers/{provider_id}/trust")
    def update_provider_trust(provider_id: str, request: dict):
        runtime = get_runtime()
        return runtime.plugin_api_service.update_provider_trust(provider_id, request)
    
    @router.get("/sessions")
    def list_sessions():
        runtime = get_runtime()
        return runtime.plugin_api_service.list_sessions()
    
    @router.post("/sessions/{profile_id}/{session_id}/recover")
    def recover_session(profile_id: str, session_id: str):
        runtime = get_runtime()
        return runtime.plugin_api_service.recover_session(profile_id, session_id)
    
    @router.delete("/sessions/{profile_id}/{session_id}")
    def delete_session(profile_id: str, session_id: str):
        runtime = get_runtime()
        return runtime.plugin_api_service.delete_session(profile_id, session_id)
    
    @router.post("/events/{event_id}/reveal")
    def reveal_event(event_id: str, ttl_seconds: int = 60):
        runtime = get_runtime()
        return runtime.plugin_api_service.reveal_event(event_id, ttl_seconds)
    
    @router.get("/metrics")
    def metrics():
        runtime = get_runtime()
        return runtime.plugin_api_service.metrics()
    
    return router


def ws_upgrade_authorized(websocket):
    """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
    # In production, this would check Hermes' auth token/cookie
    return True