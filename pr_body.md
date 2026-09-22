## Summary

Implements reversible PII protection for Hermes external LLM calls via a local Gaze sidecar.

## Changes (15 tasks)

| Task | Commit | Description |
|------|--------|-------------|
| 2 | 9fdacda | Scaffold Hermes privacy plugin (plugin.yaml, config, provider policy) |
| 3 | 3a64a39 | Authenticated Gaze sidecar shell (health, auth, status) |
| 4 | 260e19a | Gaze policy engine, profile overrides, NER provisioning |
| 5 | 06af2d7 | Encrypted session persistence (ChaCha20-Poly1305) |
| 6 | 554b695 | Transactional REST API (clean, restore, policies, sessions, metrics) |
| 7 | 1970a87 | WebSocket streaming restoration (/v1/streams) |
| 8 | 90d7f46 | Native sidecar lifecycle manager (host/profile scope, external/docker modes) |
| 9 | f11b72c | Provider request/response adapters (4 wire formats, mandatory opaque blocking) |
| 10 | 8df42cf | Privacy middleware (llm_execution, llm_stream_text, capability gating) |
| 11 | 2c37913 | Desktop plugin API (status, events, policies, providers, sessions, reveal) |
| 12/13 | e328d15 | Desktop workspace (7 tabs) + policy controls |
| 14 | 65f3f28 | CI/CD (5-target matrix, Docker, release manifest, smoke tests) |
| 15 | 95cc234 | E2E tests, documentation, release gate |

## Test Results

- **Python**: 83 passed (unit + e2e)
- **Rust**: 62 passed (cargo test)
- **Desktop ESM**: 9 passed (node --test)
- **Code quality**: cargo fmt ✓, clippy -D warnings ✓
- **Docker**: compose config OK

## Test Coverage

- External providers never receive raw PII (12 synthetic cases)
- Tool call arguments restored before execution
- Sidecar restart recovers with correct key, blocks wrong key
- Profile namespaces isolated (host vs profile scope)
- Logs never contain secrets/tokens/PII
- Mandatory mode blocks external when capabilities missing
- Compatibility mode warns but allows

## Documentation

- README.md, docs/install.md, docs/architecture.md, docs/debugging.md, docs/threat-model.md, SECURITY.md

## Breaking Changes

None (new plugin, no existing users)

## Checklist

- [x] All tests pass (83 Python + 62 Rust + 9 Desktop)
- [x] cargo fmt --check ✓
- [x] cargo clippy -D warnings ✓
- [x] node --check desktop/plugin.js ✓
- [x] docker compose config ✓