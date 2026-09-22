"""Gaze Hermes privacy plugin package."""

from gaze_privacy.errors import PrivacyBlockedError
from gaze_privacy.provider_policy import ProtectionDecision, ProviderPolicy
from gaze_privacy.secrets import ensure_secret
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus, StreamClient
from gaze_privacy.sidecar_manager import SidecarManager, ManagedSidecar
from gaze_privacy.release_manifest import ReleaseManifest, Artifact, ManifestError
from gaze_privacy.events import PrivacyEvent, EventBuffer, create_event
from gaze_privacy.runtime import PrivacyRuntime, HermesCapabilities, StreamRegistry
from gaze_privacy.middleware import init_runtime, get_runtime, llm_execution_middleware, llm_stream_text_middleware
from gaze_privacy.plugin_api_service import PluginApiService
from gaze_privacy.reveal import RevealService, RevealGrant, RevealExpired, RevealConsumed, RevealProfileMismatch, RevealMissing

__all__ = [
    "PrivacyBlockedError",
    "ProtectionDecision",
    "ProviderPolicy",
    "ensure_secret",
    "SidecarClient",
    "SidecarStatus",
    "StreamClient",
    "SidecarManager",
    "ManagedSidecar",
    "ReleaseManifest",
    "Artifact",
    "ManifestError",
    "PrivacyEvent",
    "EventBuffer",
    "create_event",
    "PrivacyRuntime",
    "HermesCapabilities",
    "StreamRegistry",
    "init_runtime",
    "get_runtime",
    "llm_execution_middleware",
    "llm_stream_text_middleware",
    "PluginApiService",
    "RevealService",
    "RevealGrant",
    "RevealExpired",
    "RevealConsumed",
    "RevealProfileMismatch",
    "RevealMissing",
]
