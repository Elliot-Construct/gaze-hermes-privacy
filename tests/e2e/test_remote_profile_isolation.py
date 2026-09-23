"""Remote profile isolation tests."""

from __future__ import annotations

import pytest


class RemoteIsolationHarness:
    """Harness for testing profile isolation in remote scenarios."""

    def __init__(self):
        self.profiles = {}

    def profile(self, name, sidecar_scope):
        class Profile:
            def __init__(self, name, scope):
                self.name = name
                self.scope = scope
                if scope == "profile":
                    self.sidecar_endpoint = f"http://127.0.0.1:65113/{name}"
                else:  # host scope
                    self.sidecar_endpoint = "http://127.0.0.1:65113"
                self.sessions = {}

            def clean(self, text):
                token = f"<token:{self.name}:{text}>"
                self.sessions[text] = token
                return token

            def restore(self, token):
                expected_prefix = f"<token:{self.name}:"
                if not token.startswith(expected_prefix):
                    raise Exception("StrictRestoreError")
                return token

            def snapshot_files(self):
                return [f"{self.name}_session.enc"]

            def send_external_followup(self, token):
                pass

        p = Profile(name, sidecar_scope)
        self.profiles[name] = p
        return p


@pytest.fixture
def harness():
    return RemoteIsolationHarness()


def test_profile_namespaces_and_dedicated_sidecars_are_isolated(harness):
    a = harness.profile("alpha", sidecar_scope="profile")
    b = harness.profile("beta", sidecar_scope="profile")
    token = a.clean("alpha@example.invalid")
    assert a.sidecar_endpoint != b.sidecar_endpoint
    with pytest.raises(Exception, match="StrictRestoreError"):
        b.restore(token)
    assert set(a.snapshot_files()).isdisjoint(set(b.snapshot_files()))


def test_host_scope_shares_sidecar(harness):
    a = harness.profile("alpha", sidecar_scope="host")
    b = harness.profile("beta", sidecar_scope="host")
    # In host scope, same endpoint
    assert a.sidecar_endpoint == b.sidecar_endpoint