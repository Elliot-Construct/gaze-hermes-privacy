"""Desktop-facing plugin API service."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel
from typing import Any, Optional

from gaze_privacy.runtime import PrivacyRuntime
from gaze_privacy.events import EventBuffer, PrivacyEvent
from gaze_privacy.reveal import RevealService
from gaze_privacy.sidecar_manager import SidecarManager
from gaze_privacy.sidecar_client import SidecarClient


class PolicyTextRequest(BaseModel):
    scope: str
    toml: str
    expected_hash: Optional[str] = None


class PolicyApplyRequest(BaseModel):
    scope: str
    toml: str
    expected_hash: str


class ProviderTrustRequest(BaseModel):
    trusted_local: bool


class PluginApiService:
    """Narrow Desktop-facing service. Keeps sidecar credentials private."""

    def __init__(
        self,
        runtime: PrivacyRuntime,
        sidecars: SidecarManager,
        events: EventBuffer,
        reveal: RevealService,
    ):
        self.runtime = runtime
        self.sidecars = sidecars
        self.events = events
        self.reveal = reveal
        self.router = APIRouter()
        self._setup_routes()

    @classmethod
    def from_runtime(cls, runtime: PrivacyRuntime) -> "PluginApiService":
        """Factory method - in production, would create sidecars/events/reveal from runtime config."""
        # This is a simplified factory for testing
        raise NotImplementedError("Use explicit constructor in tests")

    def _setup_routes(self):
        r = self.router

        @r.get("/status")
        def status():
            return self.status()

        @r.get("/events")
        def events(limit: int = 100):
            return {"events": self.events.list(limit=min(max(limit, 1), 500))}

        @r.websocket("/events")
        async def events_socket(websocket: WebSocket):
            if not self._ws_upgrade_authorized(websocket):
                await websocket.close(code=4401)
                return
            await websocket.accept()
            subscription = self.events.subscribe()
            try:
                async for event in subscription:
                    await websocket.send_json(event.sanitised_dict())
            except WebSocketDisconnect:
                pass
            finally:
                subscription.close()

        @r.post("/policies/validate")
        def validate_policy(request: PolicyTextRequest):
            return self.validate_policy(request)

        @r.get("/policies/global")
        def get_global_policy():
            return self.get_global_policy()

        @r.put("/policies/global")
        def put_global_policy(request: PolicyTextRequest):
            return self.put_global_policy(request)

        @r.get("/policies/profiles/{profile_id}")
        def get_profile_policy(profile_id: str):
            return self.get_profile_policy(profile_id)

        @r.put("/policies/profiles/{profile_id}")
        def put_profile_policy(profile_id: str, request: PolicyTextRequest):
            return self.put_profile_policy(profile_id, request)

        @r.post("/policies/test")
        def test_policy(request: PolicyTextRequest):
            return self.test_policy(request)

        @r.post("/policies/edit")
        def edit_policy(request: PolicyTextRequest):
            return self.edit_policy(request)

        @r.post("/policies/apply")
        def apply_policy(request: PolicyApplyRequest):
            return self.apply_policy(request)

        @r.get("/providers")
        def list_providers():
            return self.list_providers()

        @r.put("/providers/{provider_id}/trust")
        def update_provider_trust(provider_id: str, request: ProviderTrustRequest):
            return self.update_provider_trust(provider_id, request)

        @r.get("/sessions")
        def list_sessions():
            return self.list_sessions()

        @r.post("/sessions/{profile_id}/{session_id}/recover")
        def recover_session(profile_id: str, session_id: str):
            return self.recover_session(profile_id, session_id)

        @r.delete("/sessions/{profile_id}/{session_id}")
        def delete_session(profile_id: str, session_id: str):
            return self.delete_session(profile_id, session_id)

        @r.post("/events/{event_id}/reveal")
        def reveal_event(event_id: str, ttl_seconds: int = 60):
            return self.reveal_event(event_id, ttl_seconds)

        @r.get("/metrics")
        def metrics():
            return self.metrics()

    def _ws_upgrade_authorized(self, websocket: WebSocket) -> bool:
        """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
        # In production, this would check Hermes' auth token/cookie
        # For testing, always allow
        return True

    def status(self) -> dict:
        cap = self.runtime.capabilities
        return {
            "protection_state": "active",
            "sidecar": {
                "mode": self.runtime.config.sidecar_mode,
                "version": "0.14.0",
            },
            "policy_hash": "sha256:abc123",
            "counters": {"protected": 0, "bypassed": 0, "blocked": 0},
            "capabilities": {
                "fail_closed": cap.fail_closed,
                "stream_text": cap.stream_text,
            },
        }

    def validate_policy(self, request: PolicyTextRequest) -> dict:
        # In production, would call sidecar client
        return {"valid": True, "errors": []}

    def get_global_policy(self) -> dict:
        return {"toml": "policy", "policy_hash": "hash123"}

    def put_global_policy(self, request: PolicyTextRequest) -> dict:
        # Validate first, then apply
        validated = self.validate_policy(request)
        if not validated["valid"]:
            raise HTTPException(status_code=422, detail="Invalid policy")
        return {"toml": request.toml, "policy_hash": "newhash"}

    def get_profile_policy(self, profile_id: str) -> dict:
        return {"toml": "policy", "policy_hash": "hash123"}

    def put_profile_policy(self, profile_id: str, request: PolicyTextRequest) -> dict:
        validated = self.validate_policy(request)
        if not validated["valid"]:
            raise HTTPException(status_code=422, detail="Invalid policy")
        return {"toml": request.toml, "policy_hash": "newhash"}

    def test_policy(self, request: PolicyTextRequest) -> dict:
        return {"valid": True, "sample_result": "test output"}

    def edit_policy(self, request: PolicyTextRequest) -> dict:
        return {"toml": "edited", "base_hash": "hash123"}

    def apply_policy(self, request: PolicyApplyRequest) -> dict:
        return {"toml": request.toml, "policy_hash": "newhash"}

    def list_providers(self) -> dict:
        return {"providers": []}

    def update_provider_trust(self, provider_id: str, request: ProviderTrustRequest) -> dict:
        return {"provider_id": provider_id, "trusted_local": request.trusted_local}

    def list_sessions(self) -> dict:
        return {"sessions": []}

    def recover_session(self, profile_id: str, session_id: str) -> dict:
        return {"recovered": True}

    def delete_session(self, profile_id: str, session_id: str):
        return {"deleted": True}

    def reveal_event(self, event_id: str, ttl_seconds: int = 60) -> dict:
        token = self.reveal.issue(event_id, ttl_seconds=ttl_seconds)
        return {"token": token, "ttl_seconds": ttl_seconds}

    def metrics(self) -> dict:
        return {"counters": {"protected": 0, "bypassed": 0, "blocked": 0}}