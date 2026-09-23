"""Privacy runtime: execution, streaming, capabilities, and stream registry."""

from __future__ import annotations

import asyncio
import threading
from dataclasses import dataclass
from typing import Any, Callable, Optional

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


@dataclass
class StreamReservation:
    profile_id: str
    namespace: dict[str, str]
    stream: StreamClient


class StreamRegistry:
    """Thread-safe registry for streaming restoration clients."""

    def __init__(self):
        self._streams: dict[tuple[str, str], StreamReservation] = {}
        self._lock = threading.Lock()

    def reserve(self, namespace: dict[str, str], client: StreamClient) -> tuple[str, str]:
        key = (
            str(namespace.get("session_id", "")),
            str(namespace.get("request_id", "")),
        )
        with self._lock:
            if key not in self._streams:
                self._streams[key] = StreamReservation(
                    profile_id=namespace.get("profile_id", "default"),
                    namespace=namespace,
                    stream=client,
                )
        return key

    def get_or_open(self, key: tuple[str, str]) -> StreamClient:
        with self._lock:
            if key not in self._streams:
                raise KeyError(f"Stream key not found: {key}")
            return self._streams[key].stream

    def finish(self, key: tuple[str, str], bridge) -> None:
        with self._lock:
            reservation = self._streams.get(key)
        if reservation is None:
            return
        # Create a coroutine that calls the stream's finish method
        async def _finish_wrapper():
            result = reservation.stream.finish()
            if asyncio.iscoroutine(result):
                return await result
            return result
        coro = _finish_wrapper()
        tail = bridge.call(coro)
        if tail:
            raise PrivacyBlockedError("stream finished with unexpected buffered output")

    def abort(self, key: tuple[str, str], bridge) -> None:
        with self._lock:
            reservation = self._streams.get(key)
        if reservation is not None:
            async def _abort_wrapper():
                result = reservation.stream.abort()
                if asyncio.iscoroutine(result):
                    return await result
                return result
            coro = _abort_wrapper()
            bridge.call(coro)

    def release(self, key: tuple[str, str]) -> None:
        with self._lock:
            self._streams.pop(key, None)


class AsyncBridge:
    """Dedicated asyncio event loop running in a background thread."""

    def __init__(self):
        self._loop = asyncio.new_event_loop()
        self._ready = threading.Event()
        self._thread = threading.Thread(
            target=self._run,
            name="gaze-privacy-async",
            daemon=True,
        )
        self._thread.start()
        self._ready.wait(timeout=5.0)

    def _run(self):
        asyncio.set_event_loop(self._loop)
        self._ready.set()
        self._loop.run_forever()

    def call(self, coro, *, timeout=30.0):
        if threading.current_thread() is self._thread:
            raise RuntimeError("AsyncBridge.call() invoked from its own event-loop thread")
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=timeout)

    def close(self):
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(timeout=5.0)


@dataclass(frozen=True)
class HermesCapabilities:
    fail_closed: bool
    stream_text: bool


class PrivacyRuntime:
    """Main privacy runtime orchestrating cleaning, streaming, and events."""

    def __init__(
        self,
        config,
        provider_policy,
        capabilities,
        sidecars,
        events,
    ):
        self.config = config
        self.provider_policy = provider_policy
        self.capabilities = capabilities
        self.sidecars = sidecars
        self.events = events
        self.streams = StreamRegistry()
        self.bridge = AsyncBridge()
        self.plugin_api_service = None  # Set by init_runtime

    def resolve_profile_id(self) -> str:
        """Resolve the actual Hermes profile name."""
        try:
            from hermes_constants import get_hermes_home, profile_name_for_home
            return profile_name_for_home(get_hermes_home()) or "default"
        except Exception:
            return "default"

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

        profile_id = self.resolve_profile_id()
        managed = await self.sidecars.ensure_running(profile_id)

        namespace = {
            "profile_id": profile_id,
            "session_id": context.get("session_id", ""),
            "request_id": context.get("api_request_id", ""),
        }

        prepared = extract_request_fields(api_mode, request, mandatory=self.config.mandatory_mode)
        cleaned = await managed.client.clean(namespace, prepared.fields)
        protected_request = prepared.apply(cleaned["fields"])

        stream_client = await managed.client.open_stream(namespace)
        request_key = self.streams.reserve(namespace, client=stream_client)
        try:
            response = next_call(protected_request)
            restored = await restore_completed_response(
                managed.client,
                namespace,
                api_mode,
                response,
            )
            self.streams.finish(request_key, self.bridge)
            return restored
        except Exception:
            self.streams.abort(request_key, self.bridge)
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
        """Synchronous execution for Hermes middleware.
        
        CRITICAL: This method runs synchronously on the Hermes thread.
        Only sidecar I/O operations are delegated to the AsyncBridge thread.
        The `next_call` provider callback runs on the Hermes thread.
        """
        from gaze_privacy.provider_policy import ProtectionDecision

        if self.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
            self.events.add(
                create_event(
                    provider=provider,
                    api_mode=api_mode,
                    session_id=context.get("session_id", ""),
                    profile_id=self.resolve_profile_id(),
                    request_id=context.get("api_request_id", ""),
                    state="bypass",
                )
            )
            return next_call(request)

        self.require_or_mark_capabilities(provider=provider, context=context)

        profile_id = self.resolve_profile_id()
        managed = self.bridge.call(self.sidecars.ensure_running(profile_id))

        namespace = {
            "profile_id": profile_id,
            "session_id": context.get("session_id", ""),
            "request_id": context.get("api_request_id", ""),
        }

        prepared = extract_request_fields(api_mode, request, mandatory=self.config.mandatory_mode)
        cleaned = self.bridge.call(managed.client.clean(namespace, prepared.fields))
        protected_request = prepared.apply(cleaned["fields"])

        stream = self.bridge.call(managed.client.open_stream(namespace))
        key = self.streams.reserve(namespace, client=stream)
        try:
            # CRITICAL: next_call runs on the Hermes thread (current thread)
            response = next_call(protected_request)
            
            restored = self.bridge.call(restore_completed_response(
                managed.client,
                namespace,
                api_mode,
                response,
            ))
            self.streams.finish(key, self.bridge)
            return restored
        except Exception:
            self.streams.abort(key, self.bridge)
            raise
        finally:
            self.streams.release(key)

    async def stream_text(
        self,
        *,
        text: str,
        kind: str,
        provider: str,
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
        session_id: str,
        api_request_id: str,
        **_ctx: Any,
    ) -> dict[str, str]:
        """Synchronous streaming text restoration for Hermes middleware."""
        key = (str(session_id or ""), str(api_request_id or ""))
        stream = self.streams.get_or_open(key)
        restored = self.bridge.call(stream.feed(kind=kind, text=text))
        return {"text": restored}


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