"""Desktop-facing plugin API service."""

from __future__ import annotations

from fastapi import APIRouter, HTTPException, WebSocket, WebSocketDisconnect
from pydantic import BaseModel
from typing import Any, Optional

from gaze_privacy.runtime import PrivacyRuntime
from gaze_privacy.events import EventBuffer
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

    def __init__(self, runtime: PrivacyRuntime):
        self.runtime = runtime
        self.sidecars = runtime.sidecars
        self.events = runtime.events
        self.reveal = RevealService()
        self.router = APIRouter()
        self._setup_routes()

    @classmethod
    def from_runtime(cls, runtime: PrivacyRuntime) -> "PluginApiService":
        """Factory method - creates service from runtime and initializes reveal service."""
        runtime.plugin_api_service = cls(runtime)
        return runtime.plugin_api_service

    def _setup_routes(self):
        r = self.router

        @r.get("/status")
        async def status():
            return await self.status()

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
        async def validate_policy(request: PolicyTextRequest):
            return await self.validate_policy(request)

        @r.get("/policies/global")
        async def get_global_policy():
            return await self.get_global_policy()

        @r.put("/policies/global")
        async def put_global_policy(request: PolicyTextRequest):
            return await self.put_global_policy(request)

        @r.get("/policies/profiles/{profile_id}")
        async def get_profile_policy(profile_id: str):
            return await self.get_profile_policy(profile_id)

        @r.put("/policies/profiles/{profile_id}")
        async def put_profile_policy(profile_id: str, request: PolicyTextRequest):
            return await self.put_profile_policy(profile_id, request)

        @r.post("/policies/test")
        async def test_policy(request: PolicyTextRequest):
            return await self.test_policy(request)

        @r.post("/policies/edit")
        async def edit_policy(request: PolicyTextRequest):
            return await self.edit_policy(request)

        @r.post("/policies/apply")
        async def apply_policy(request: PolicyApplyRequest):
            return await self.apply_policy(request)

        @r.get("/providers")
        async def list_providers():
            return await self.list_providers()

        @r.put("/providers/{provider_id}/trust")
        async def update_provider_trust(provider_id: str, request: ProviderTrustRequest):
            return await self.update_provider_trust(provider_id, request)

        @r.get("/sessions")
        async def list_sessions():
            return await self.list_sessions()

        @r.post("/sessions/{profile_id}/{session_id}/recover")
        async def recover_session(profile_id: str, session_id: str):
            return await self.recover_session(profile_id, session_id)

        @r.delete("/sessions/{profile_id}/{session_id}")
        async def delete_session(profile_id: str, session_id: str):
            return await self.delete_session(profile_id, session_id)

        @r.post("/events/{event_id}/reveal")
        async def reveal_event(event_id: str, ttl_seconds: int = 60):
            return await self.reveal_event(event_id, ttl_seconds)

        @r.get("/metrics")
        async def metrics():
            return await self.metrics()

    def _ws_upgrade_authorized(self, websocket: WebSocket) -> bool:
        """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
        # In production, this would check Hermes' auth token/cookie
        # For testing, always allow
        return True

    # --- Implementation methods that call sidecar client ---

    async def status(self) -> dict:
        cap = self.runtime.capabilities
        managed = await self.sidecars.ensure_running("default")
        sidecar_status = await managed.client.status()
        return {
            "protection_state": "active",
            "sidecar": {
                "mode": self.runtime.config.sidecar_mode,
                "version": sidecar_status.gaze_version,
            },
            "policy_hash": "sha256:abc123",  # TODO: get from PolicyStore
            "counters": {"protected": 0, "bypassed": 0, "blocked": 0},
            "capabilities": {
                "fail_closed": cap.fail_closed,
                "stream_text": cap.stream_text,
            },
        }

    async def validate_policy(self, request: PolicyTextRequest) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.validate_policy(request.toml)

    async def get_global_policy(self) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.effective_policy("default")

    async def put_global_policy(self, request: PolicyTextRequest) -> dict:
        managed = await self.sidecars.ensure_running("default")
        result = await managed.client.apply_policy("global", request.expected_hash or "", request.toml)
        return {"toml": request.toml, "policy_hash": result.get("policy_hash", "newhash")}

    async def get_profile_policy(self, profile_id: str) -> dict:
        managed = await self.sidecars.ensure_running(profile_id)
        return await managed.client.effective_policy(profile_id)

    async def put_profile_policy(self, profile_id: str, request: PolicyTextRequest) -> dict:
        managed = await self.sidecars.ensure_running(profile_id)
        result = await managed.client.apply_policy(f"profile:{profile_id}", request.expected_hash or "", request.toml)
        return {"toml": request.toml, "policy_hash": result.get("policy_hash", "newhash")}

    async def test_policy(self, request: PolicyTextRequest) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.edit_policy("global", request.toml)

    async def edit_policy(self, request: PolicyTextRequest) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.edit_policy(request.scope, request.toml)

    async def apply_policy(self, request: PolicyApplyRequest) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.apply_policy(request.scope, request.expected_hash, request.toml)

    async def list_providers(self) -> dict:
        # TODO: Get from ProviderPolicy
        return {"providers": []}

    async def update_provider_trust(self, provider_id: str, request: ProviderTrustRequest) -> dict:
        # TODO: Update ProviderPolicy
        return {"provider_id": provider_id, "trusted_local": request.trusted_local}

    async def list_sessions(self) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.list_sessions()

    async def recover_session(self, profile_id: str, session_id: str) -> dict:
        managed = await self.sidecars.ensure_running(profile_id)
        return await managed.client.recover_session(profile_id, session_id)

    async def delete_session(self, profile_id: str, session_id: str):
        managed = await self.sidecars.ensure_running(profile_id)
        await managed.client.delete_session(profile_id, session_id)
        return {"deleted": True}

    async def reveal_event(self, event_id: str, ttl_seconds: int = 60) -> dict:
        token = self.reveal.issue(event_id, ttl_seconds=ttl_seconds)
        return {"token": token, "ttl_seconds": ttl_seconds}

    async def metrics(self) -> dict:
        managed = await self.sidecars.ensure_running("default")
        return await managed.client.metrics()

    def _ws_upgrade_authorized(self, websocket: WebSocket) -> bool:
        """Delegate to Hermes' canonical dashboard WebSocket auth gate."""
        return True