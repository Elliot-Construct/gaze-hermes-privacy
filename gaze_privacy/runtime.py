"""Privacy runtime: execution, streaming, capabilities, and stream registry."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Callable

from gaze_privacy.events import EventBuffer, PrivacyEvent, create_event
from gaze_privacy.sidecar_client import SidecarClient, StreamClient
from gaze_privacy.adapters import (
    extract_request_fields,
    restore_completed_response,
    TextField,
)
from gaze_privacy.errors import PrivacyBlockedError


@dataclass(frozen=True)
class HermesCapabilities:
    fail_closed: bool
    stream_text: bool


class StreamRegistry:
    """Thread-safe registry for streaming restoration clients."""

    def __init__(self):
        self._streams: dict[tuple[str, str, str], StreamClient] = {}
        self._lock = __import__("threading").Lock()

    def reserve(self, namespace: dict[str, str], client: StreamClient) -> tuple[str, str, str]:
        key = (
            str(namespace.get("profile_id", "default")),
            str(namespace.get("session_id", "")),
            str(namespace.get("request_id", "")),
        )
        with self._lock:
            if key not in self._streams:
                self._streams[key] = client
        return key

    def get_or_open(self, key: tuple[str, str, str]) -> StreamClient:
        with self._lock:
            if key not in self._streams:
                raise KeyError(f"Stream key not found: {key}")
            return self._streams[key]

    def finish_if_open(self, key: tuple[str, str, str]) -> None:
        with self._lock:
            if key in self._streams:
                # Stream will be cleaned up on release
                pass

    def abort_if_open(self, key: tuple[str, str, str]) -> None:
        with self._lock:
            if key in self._streams:
                client = self._streams[key]
                if hasattr(client, "abort"):
                    import asyncio
                    try:
                        loop = asyncio.get_event_loop()
                        if loop.is_running():
                            loop.create_task(client.abort())
                        else:
                            loop.run_until_complete(client.abort())
                    except Exception:
                        pass

    def release(self, key: tuple[str, str, str]) -> None:
        with self._lock:
            self._streams.pop(key, None)


class PrivacyRuntime:
    """Main privacy runtime orchestrating cleaning, streaming, and events."""

    def __init__(
        self,
        config,
        provider_policy,
        capabilities: HermesCapabilities,
        sidecars,
        events: EventBuffer,
    ):
        self.config = config
        self.provider_policy = provider_policy
        self.capabilities = capabilities
        self.sidecars = sidecars
        self.events = events
        self.streams = StreamRegistry()
        self.plugin_api_service = None  # Set by init_runtime

    def require_or_mark_capabilities(self, *, provider: str, context: dict) -> None:
        if self.capabilities.fail_closed and self.capabilities.stream_text:
            return
        if self.config.compatibility_mode:
            self.events.add(
                create_event(
                    provider=provider,
                    api_mode=context.get("api_mode", ""),
                    session_id=context.get("session_id", ""),
                    profile_id=context.get("profile_id", "default"),
                    request_id=context.get("api_request_id", ""),
                    state="protection_not_guaranteed",
                    error_code="hermes_capability_missing",
                )
            )
            return
        raise PrivacyBlockedError("Hermes lacks required fail-closed privacy capabilities")

    async def execute(
        self,
        *,
        request: dict[str, Any],
        next_call: Callable[[dict[str, Any]], Any],
        provider: str,
        api_mode: str,
        **context: Any,
    ) -> Any:
        """Async execution for tests and async contexts."""
        from gaze_privacy.provider_policy import ProtectionDecision

        if self.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
            self.events.add(
                create_event(
                    provider=provider,
                    api_mode=api_mode,
                    session_id=context.get("session_id", ""),
                    profile_id=context.get("profile_id", "default"),
                    request_id=context.get("api_request_id", ""),
                    state="bypass",
                )
            )
            return next_call(request)

        self.require_or_mark_capabilities(provider=provider, context=context)

        profile_id = context.get("profile_id") or context.get("turn_id") or "default"
        managed = await self.sidecars.ensure_running(profile_id)

        namespace = {
            "profile_id": profile_id,
            "session_id": context.get("session_id", ""),
            "request_id": context.get("api_request_id", ""),
        }

        prepared = extract_request_fields(api_mode, request, mandatory=self.config.mandatory_mode)
        cleaned = await managed.client.clean(namespace, prepared.fields)
        protected_request = prepared.apply(cleaned["fields"])

        request_key = self.streams.reserve(namespace, client=managed.client)
        try:
            response = next_call(protected_request)
            restored = await restore_completed_response(
                managed.client,
                namespace,
                api_mode,
                response,
            )
            self.streams.finish_if_open(request_key)
            return restored
        except Exception:
            self.streams.abort_if_open(request_key)
            raise
        finally:
            self.streams.release(request_key)

    def execute_sync(
        self,
        *,
        request: dict[str, Any],
        next_call: Callable[[dict[str, Any]], Any],
        provider: str,
        api_mode: str,
        **context: Any,
    ) -> Any:
        """Synchronous execution for Hermes middleware."""
        import asyncio
        return asyncio.run(self.execute(
            request=request,
            next_call=next_call,
            provider=provider,
            api_mode=api_mode,
            **context,
        ))

    async def stream_text(
        self,
        *,
        text: str,
        kind: str,
        provider: str,
        profile_id: str,
        session_id: str,
        api_request_id: str,
        **_ctx: Any,
    ) -> dict[str, str]:
        """Async streaming text restoration for tests and async contexts."""
        from gaze_privacy.provider_policy import ProtectionDecision

        if self.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
            return {"text": text}

        self.require_or_mark_capabilities(provider=provider, context=_ctx)

        key = (
            str(profile_id or "default"),
            str(session_id or ""),
            str(api_request_id or ""),
        )
        stream = self.streams.get_or_open(key)
        restored = await stream.feed(kind=kind, text=text)
        return {"text": restored}

    def stream_text_sync(
        self,
        *,
        text: str,
        kind: str,
        provider: str,
        profile_id: str,
        session_id: str,
        api_request_id: str,
        **_ctx: Any,
    ) -> dict[str, str]:
        """Synchronous streaming text restoration for Hermes middleware."""
        import asyncio
        return asyncio.run(self.stream_text(
            text=text,
            kind=kind,
            provider=provider,
            profile_id=profile_id,
            session_id=session_id,
            api_request_id=api_request_id,
        ))


def llm_execution_middleware(**kwargs):
    # This will be bound to a runtime instance
    raise NotImplementedError("Use PrivacyRuntime.execute directly")


def llm_stream_text_middleware(
    *,
    text: str,
    kind: str,
    provider: str,
    profile_id: str,
    session_id: str,
    api_request_id: str,
    **_ctx,
) -> dict[str, str]:
    raise NotImplementedError("Use PrivacyRuntime stream method directly")