from gaze_privacy.provider_policy import ProviderPolicy, ProtectionDecision


def test_empty_trust_set_protects_everything():
    policy = ProviderPolicy(frozenset())
    for provider_id in ("openrouter", "anthropic", "ollama", "local-vllm", ""):
        assert policy.classify(provider_id) is ProtectionDecision.PROTECT


def test_exact_match_only_bypass():
    policy = ProviderPolicy(frozenset({"local-vllm"}))
    assert policy.classify("local-vllm") is ProtectionDecision.BYPASS
    assert policy.classify("local-vllm ") is ProtectionDecision.PROTECT
    assert policy.classify("Local-vLLM") is ProtectionDecision.PROTECT
    assert policy.classify("http://127.0.0.1:11434") is ProtectionDecision.PROTECT


def test_url_like_ids_not_inferring_trust():
    policy = ProviderPolicy(frozenset({"local-vllm"}))
    assert policy.classify("http://127.0.0.1:11434") is ProtectionDecision.PROTECT
    assert policy.classify("localhost") is ProtectionDecision.PROTECT
    assert policy.classify("ollama") is ProtectionDecision.PROTECT
