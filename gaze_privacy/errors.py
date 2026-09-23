"""Typed errors for the privacy boundary."""


class PrivacyBlockedError(Exception):
    """External provider transmission blocked because protection cannot be established."""
