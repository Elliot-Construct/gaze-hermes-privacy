from pathlib import Path

from gaze_privacy.config import PrivacyConfig
from gaze_privacy.provider_policy import ProviderPolicy, ProtectionDecision


def test_external_provider_is_protected_by_default(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    cfg = PrivacyConfig.load()
    policy = ProviderPolicy(cfg.trusted_local_providers)
    assert policy.classify("openrouter") == ProtectionDecision.PROTECT


def test_only_explicit_provider_id_bypasses(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    (tmp_path / "config.toml").write_text(
        '[providers]\ntrusted_local = ["local-vllm"]\n',
        encoding="utf-8",
    )
    cfg = PrivacyConfig.load()
    policy = ProviderPolicy(cfg.trusted_local_providers)
    assert policy.classify("local-vllm") == ProtectionDecision.BYPASS
    assert policy.classify("ollama") == ProtectionDecision.PROTECT
    assert policy.classify("http://127.0.0.1:11434") == ProtectionDecision.PROTECT


def test_config_defaults(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    cfg = PrivacyConfig.load()
    assert cfg.home == Path(tmp_path)
    assert cfg.sidecar_mode == "native"
    assert cfg.sidecar_scope == "host"
    assert cfg.sidecar_url == "http://127.0.0.1:65113"
    assert cfg.mandatory_mode is True
    assert cfg.compatibility_mode is False
    assert cfg.trusted_local_providers == frozenset()
    assert cfg.api_token_file == Path(tmp_path) / "secrets" / "api-token"
    assert cfg.master_key_file == Path(tmp_path) / "secrets" / "snapshot-key"
    assert cfg.global_policy_file == Path(tmp_path) / "policies" / "global.toml"
    assert cfg.profile_policy_dir == Path(tmp_path) / "policies" / "profiles"


def test_invalid_sidecar_scope_rejected(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    (tmp_path / "config.toml").write_text(
        '[sidecar]\nscope = "rack"\n',
        encoding="utf-8",
    )
    try:
        PrivacyConfig.load()
    except ValueError as exc:
        assert "scope" in str(exc)
    else:
        raise AssertionError("expected ValueError for invalid sidecar.scope")
