"""Desktop plugin backend API for Gaze Privacy."""

from fastapi import APIRouter, HTTPException, WebSocket, WebSocketDisconnect
from fastapi.responses import JSONResponse

from gaze_privacy.middleware import get_runtime
from gaze_privacy.plugin_api_service import PluginApiService

router = APIRouter()


def _service() -> PluginApiService:
    """Get the plugin API service, raising appropriate errors if unavailable."""
    try:
        runtime = get_runtime()
    except RuntimeError as exc:
        raise HTTPException(
            status_code=503,
            detail="Gaze privacy runtime is not initialized",
        ) from exc

    service = runtime.plugin_api_service
    if service is None:
        raise HTTPException(
            status_code=503,
            detail="Gaze privacy API service is unavailable",
        )

    return service


def _ws_upgrade_authorized(websocket: WebSocket) -> bool:
    """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
    try:
        from hermes_cli import web_server_chat as _ws
    except Exception:
        return True
    return bool(_ws._ws_auth_ok(websocket))


@router.get("/status")
async def status():
    """Get the current privacy status."""
    return await _service().status()


@router.get("/events")
async def events(limit: int = 100):
    """Get recent privacy events."""
    return {"events": _service().events.list(limit=min(max(limit, 1), 500))}


@router.websocket("/events")
async def events_socket(websocket: WebSocket):
    """WebSocket for real-time privacy events."""
    if not _ws_upgrade_authorized(websocket):
        await websocket.close(code=4401)
        return
    await websocket.accept()
    subscription = _service().events.subscribe()
    try:
        async for event in subscription:
            await websocket.send_json(event.sanitised_dict())
    except WebSocketDisconnect:
        pass
    finally:
        subscription.close()


@router.post("/policies/validate")
async def validate_policy(request: dict):
    """Validate a policy TOML document."""
    return await _service().validate_policy(request)


@router.get("/policies/global")
async def get_global_policy():
    """Get the global policy."""
    return await _service().get_global_policy()


@router.put("/policies/global")
async def put_global_policy(request: dict):
    """Update the global policy."""
    return await _service().put_global_policy(request)


@router.get("/policies/profiles/{profile_id}")
async def get_profile_policy(profile_id: str):
    """Get a profile-specific policy."""
    return await _service().get_profile_policy(profile_id)


@router.put("/policies/profiles/{profile_id}")
async def put_profile_policy(profile_id: str, request: dict):
    """Update a profile-specific policy."""
    return await _service().put_profile_policy(profile_id, request)


@router.post("/policies/test")
async def test_policy(request: dict):
    """Test a policy with sample text."""
    return await _service().test_policy(request)


@router.post("/policies/edit")
async def edit_policy(request: dict):
    """Edit a policy and return the edited version for validation."""
    return await _service().edit_policy(request)


@router.post("/policies/apply")
async def apply_policy(request: dict):
    """Apply a policy after validation."""
    return await _service().apply_policy(request)


@router.get("/providers")
async def list_providers():
    """List all known providers and their trust status."""
    return await _service().list_providers()


@router.put("/providers/{provider_id}/trust")
async def update_provider_trust(provider_id: str, request: dict):
    """Update the trust status of a provider."""
    return await _service().update_provider_trust(provider_id, request)


@router.get("/sessions")
async def list_sessions():
    """List all active sessions."""
    return await _service().list_sessions()


@router.post("/sessions/{profile_id}/{session_id}/recover")
async def recover_session(profile_id: str, session_id: str):
    """Attempt to recover a session."""
    return await _service().recover_session(profile_id, session_id)


@router.delete("/sessions/{profile_id}/{session_id}")
async def delete_session(profile_id: str, session_id: str):
    """Delete a session."""
    await _service().delete_session(profile_id, session_id)
    return {"deleted": True}


@router.post("/events/{event_id}/reveal")
async def reveal_event(event_id: str, ttl_seconds: int = 60):
    """Request temporary reveal of sensitive data for an event."""
    return await _service().reveal_event(event_id, ttl_seconds)


@router.get("/metrics")
async def metrics():
    """Get aggregated metrics."""
    return await _service().metrics()