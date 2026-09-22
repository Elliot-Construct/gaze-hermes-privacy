"""Exact-match trusted-local provider policy. No URL/hostname inference."""

from __future__ import annotations

from enum import StrEnum


class ProtectionDecision(StrEnum):
    PROTECT = "protect"
    BYPASS = "bypass"


class ProviderPolicy:
    def __init__(self, trusted_local_providers: frozenset[str]):
        self._trusted = trusted_local_providers

    def classify(self, provider_id: str) -> ProtectionDecision:
        return (
            ProtectionDecision.BYPASS
            if provider_id in self._trusted
            else ProtectionDecision.PROTECT
        )
