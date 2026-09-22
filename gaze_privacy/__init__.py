"""Gaze Hermes privacy plugin package."""

from gaze_privacy.errors import PrivacyBlockedError
from gaze_privacy.provider_policy import ProtectionDecision, ProviderPolicy
from gaze_privacy.secrets import ensure_secret
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus, StreamClient
from gaze_privacy.sidecar_manager import SidecarManager, ManagedSidecar
from gaze_privacy.release_manifest import ReleaseManifest, Artifact, ManifestError

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
]
