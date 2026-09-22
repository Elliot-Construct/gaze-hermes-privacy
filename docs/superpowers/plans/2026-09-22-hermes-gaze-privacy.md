# Hermes Gaze Privacy Implementation Plan

**Status:** Ready for review; implementation has not started.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone open-source Hermes plugin that reversibly pseudonymises PII before external LLM calls, restores responses locally before Hermes parses them, preserves live streaming, and exposes a native Hermes Desktop privacy console.

**Architecture:** A root-level Hermes Python plugin owns provider trust, request/response adaptation, fail-closed middleware integration, and lifecycle management for a native Rust sidecar. The sidecar embeds Gaze 0.14.0 crates, owns reversible mappings and encrypted snapshots, exposes authenticated loopback REST/WebSocket APIs, and never routes LLM traffic itself. A single uncompiled Hermes Desktop `desktop/plugin.js` talks only to the Python plugin's `/api/plugins/gaze-hermes-privacy` namespace.

**Tech Stack:** Python 3.11+, Hermes Agent plugin/middleware APIs, Rust 1.89+, Gaze 0.14.0 crates, Axum/Tokio WebSockets, ChaCha20-Poly1305, TOML/toml_edit, pytest, cargo test, Hermes Desktop plugin SDK, GitHub Actions, Docker.

**Spec:** `docs/superpowers/specs/2026-09-22-hermes-gaze-privacy-design.md`

## Global Constraints

- The project is open-source and vendor-neutral; no company-specific branding, policies, paths, or assumptions.
- No permanent Hermes fork.
- External providers are protected by default; only explicitly allowlisted trusted-local providers bypass Gaze.
- No automatic trust inference from localhost, loopback IPs, Ollama, vLLM, provider names, or network placement.
- Hermes remains responsible for provider credentials, model selection, routing, retries, and fallbacks.
- The default install path requires neither Docker, a Rust compiler, nor a separate Gaze installation.
- Native sidecar API binds to `127.0.0.1:65113` by default and requires bearer authentication.
- Streaming restoration is required in v1.
- Active mappings are in memory; restart recovery uses encrypted persisted snapshots.
- Production secrets come from secret files where configured; secure generated-and-persisted fallbacks are allowed.
- Gaze NER assets are pinned and checksum-verified; manual/offline provisioning is supported.
- TOML is the canonical policy source; effective policy is global base plus optional per-profile overrides.
- The Desktop renderer never receives the sidecar bearer secret or snapshot encryption key.
- Raw PII is absent from ordinary logs and persisted debug events.
- Unsupported opaque, binary, image, audio, or unknown provider payload carriers block external transmission in mandatory mode.
- Mandatory mode blocks external providers when required fail-closed Hermes capabilities are unavailable.
- Gaze dependencies are pinned to `0.14.0` for the first implementation pass; upgrade only through an explicit compatibility change.
- Rust minimum version is `1.89`, matching Gaze 0.14.0.

## Review Focus

1. **Unknown provider payload carriers:** a request containing an unrecognised opaque/binary/multimodal field must block in mandatory mode rather than forward bytes that were not inspected. Task 9 adds the contract tests.
2. **Streaming token boundaries across lanes:** a Gaze token split across chunks, including interleaved text/reasoning lanes, must restore correctly without concatenating lane state or emitting partial token syntax. Task 7 adds the state-machine tests.
3. **Crash/corruption during snapshot persistence:** an interrupted atomic write, wrong key, tampered ciphertext, or stale temporary file must never replace a valid recoverable session. Task 5 adds the persistence tests.
4. **Provider fallback trust changes:** fallback from trusted-local to external must re-enter protection, while fallback from one external provider to another must not reuse provider trust. Task 10 adds the end-to-end middleware tests.
5. **Sensitive debug reveal lifecycle:** revealed payloads must disappear on timeout, profile change, pane unmount, and disconnect, and must never enter normal event persistence. Tasks 11 and 13 add backend and Desktop tests.

---

### Task 1: Upstream Hermes security middleware contract

**Files in upstream `NousResearch/hermes-agent`:**
- Modify: `hermes_cli/plugins.py`
- Modify: `hermes_cli/plugins_dispatch.py`
- Modify: `hermes_cli/middleware.py`
- Modify: `agent/stream_delivery.py`
- Modify: `tests/hermes_cli/test_plugins.py`
- Modify: `tests/agent/test_plugin_stream_hooks.py`
- Modify: `website/docs/developer-guide/middleware.md`
- Modify: `website/docs/developer-guide/plugins/index.md`

**Files in this repository:**
- Modify: GitHub Issue #1 description to include the stream-transform requirement
- Create: `docs/upstream-hermes.md`

**Interfaces:**
- Consumes: current Hermes `PluginContext.register_middleware(kind: str, callback: Callable)`, `run_llm_execution_middleware(request: Dict[str, Any], next_call: Callable[[Dict[str, Any]], Any], **context: Any) -> Any`, and stream delivery methods.
- Produces: `PluginContext.register_middleware(kind, callback, *, failure_mode="open")` with `failure_mode in {"open","closed"}`; new middleware kind `llm_stream_text`; `apply_llm_stream_text_middleware(text: str, *, kind: str, **context) -> str`; additive `profile_id` and `api_request_id` in stream context.

- [ ] **Step 1: Write failing fail-closed execution tests**

Add tests proving both pre- and post-`next_call` callback failures propagate when registered closed, while existing middleware remains fail-open:

```python
def test_llm_execution_failure_mode_closed_never_falls_through(monkeypatch):
    from hermes_cli.middleware import run_llm_execution_middleware
    from hermes_cli.plugins import get_plugin_manager

    manager = get_plugin_manager()
    provider_called = False

    def privacy_middleware(*, next_call, **_kwargs):
        raise RuntimeError("privacy unavailable")

    manager._middleware.setdefault("llm_execution", []).append(privacy_middleware)
    manager._middleware_failure_modes[("llm_execution", id(privacy_middleware))] = "closed"

    def provider(_request):
        nonlocal provider_called
        provider_called = True
        return {"ok": True}

    with pytest.raises(RuntimeError, match="privacy unavailable"):
        run_llm_execution_middleware({"messages": []}, provider)

    assert provider_called is False


def test_closed_middleware_failure_after_next_call_does_not_return_unrestored_result():
    from hermes_cli.middleware import run_llm_execution_middleware
    from hermes_cli.plugins import get_plugin_manager

    manager = get_plugin_manager()

    def privacy_middleware(*, next_call, request, **_kwargs):
        next_call(request)
        raise RuntimeError("restore failed")

    manager._middleware.setdefault("llm_execution", []).append(privacy_middleware)
    manager._middleware_failure_modes[("llm_execution", id(privacy_middleware))] = "closed"

    with pytest.raises(RuntimeError, match="restore failed"):
        run_llm_execution_middleware({"messages": []}, lambda _request: {"content": "<token>"})
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
pytest tests/hermes_cli/test_plugins.py -k "failure_mode_closed or closed_middleware_failure_after_next_call" -v
```

Expected: FAIL because `_middleware_failure_modes` and closed-mode handling do not exist.

- [ ] **Step 3: Add explicit middleware failure metadata**

Keep `_middleware` as the existing callback list for compatibility and add `PluginManager._middleware_failure_modes: dict[tuple[str, int], str]`, keyed by `(kind, id(callback))`. Do not invent a second middleware registry. Extend the existing registrar directly:

```python
VALID_MIDDLEWARE_FAILURE_MODES = frozenset({"open", "closed"})

def register_middleware(
    self,
    kind: str,
    callback: Callable,
    *,
    failure_mode: str = "open",
) -> PluginRegistration:
    if failure_mode not in VALID_MIDDLEWARE_FAILURE_MODES:
        raise ValueError(f"unsupported middleware failure_mode: {failure_mode!r}")
    if kind not in VALID_MIDDLEWARE:
        logger.warning(
            "Plugin '%s' registered unknown middleware '%s' (valid: %s)",
            self.manifest.name, kind, ", ".join(sorted(VALID_MIDDLEWARE)),
        )
    self._manager._middleware.setdefault(kind, []).append(callback)
    mode_key = (kind, id(callback))
    self._manager._middleware_failure_modes[mode_key] = failure_mode

    def release() -> None:
        self._manager._remove_callback(self._manager._middleware, kind, callback)
        self._manager._middleware_failure_modes.pop(mode_key, None)

    return self._track("middleware", kind, release)
```

Add `PluginManager.middleware_failure_mode(kind, callback) -> str` returning `"open"` when no explicit entry exists. `_run_execution_chain` must re-raise the middleware callback's own exception when its mode is `closed`, including after `next_call()` succeeded. Downstream exceptions continue to propagate unchanged through `_DownstreamExecutionError`.

- [ ] **Step 4: Re-run fail-closed tests and existing middleware tests**

Run:

```bash
pytest tests/hermes_cli/test_plugins.py tests/hermes_cli/test_plugin_hook_failure_reporting.py -v
```

Expected: PASS, including legacy fail-open tests.

- [ ] **Step 5: Write failing synchronous stream-transform tests**

Add `llm_stream_text` as a middleware kind and test that it transforms before the UI callback and that closed failures abort delivery:

```python
def test_stream_text_middleware_transforms_before_display(monkeypatch):
    agent = _agent()
    displayed = []
    agent.stream_delta_callback = displayed.append
    agent._current_api_request_id = "turn-1:api:1"

    monkeypatch.setattr(
        "hermes_cli.middleware.apply_llm_stream_text_middleware",
        lambda text, **ctx: "Jane" if text == "<Name_1>" else text,
    )

    agent._fire_stream_delta("<Name_1>")

    assert displayed == ["Jane"]


def test_stream_text_closed_failure_reaches_stream_caller(monkeypatch):
    agent = _agent()
    agent.stream_delta_callback = lambda _text: None

    def fail(*_args, **_kwargs):
        raise RuntimeError("stream restore failed")

    monkeypatch.setattr("hermes_cli.middleware.apply_llm_stream_text_middleware", fail)

    with pytest.raises(RuntimeError, match="stream restore failed"):
        agent._fire_stream_delta("<Name")
```

Add explicit reasoning and commentary tests so those lanes cannot bypass the synchronous transform:

```python
def test_reasoning_stream_text_middleware_transforms_before_reasoning_callback(monkeypatch):
    agent = _agent()
    delivered = []
    agent.reasoning_callback = delivered.append

    monkeypatch.setattr(
        "hermes_cli.middleware.apply_llm_stream_text_middleware",
        lambda text, *, kind, **_ctx: "Jane" if kind == "reasoning" else text,
    )

    agent._fire_reasoning_delta("<Name_1>")

    assert delivered == ["Jane"]


def test_codex_commentary_is_transformed_before_interim_delivery(monkeypatch):
    agent = _agent()
    delivered = []
    agent.interim_assistant_callback = (
        lambda text, *, already_streamed=False:
        delivered.append((text, already_streamed))
    )

    monkeypatch.setattr(
        "hermes_cli.middleware.apply_llm_stream_text_middleware",
        lambda text, *, kind, **_ctx: "Jane" if kind == "interim" else text,
    )

    agent._fire_streamed_codex_commentary("<Name_1>")

    assert delivered == [("Jane", False)]
```

Keep the existing closed-failure test on the text lane and add one parameterised test that raises from the transform for `kind in {"reasoning", "interim"}`, asserting neither callback receives untransformed text.

- [ ] **Step 6: Implement `llm_stream_text`**

In `hermes_cli/middleware.py`:

```python
LLM_STREAM_TEXT_MIDDLEWARE = "llm_stream_text"
VALID_MIDDLEWARE.add(LLM_STREAM_TEXT_MIDDLEWARE)

def apply_llm_stream_text_middleware(text: str, *, kind: str, **context: Any) -> str:
    from hermes_cli.plugins import get_plugin_manager

    manager = get_plugin_manager()
    current = text
    for callback in list(manager._middleware.get(LLM_STREAM_TEXT_MIDDLEWARE, [])):
        payload = middleware_payload(text=current, kind=kind, **context)
        try:
            result = callback(**payload)
        except Exception as exc:
            manager._report_hook_failure(
                LLM_STREAM_TEXT_MIDDLEWARE, callback, payload, exc, surface="Middleware"
            )
            if manager.middleware_failure_mode(LLM_STREAM_TEXT_MIDDLEWARE, callback) == "closed":
                raise
            continue
        if isinstance(result, dict) and isinstance(result.get("text"), str):
            current = result["text"]
    return current
```

In `agent/stream_delivery.py`, add canonical `profile_id` and `api_request_id` to `_stream_hook_base_payload()`:

```python
def _stream_hook_base_payload(self) -> Dict[str, Any]:
    from hermes_constants import get_hermes_home, profile_name_for_home
    return {
        "turn_id": getattr(self, "_current_turn_id", "") or "",
        "iteration": int(getattr(self, "_api_call_count", 0) or 0),
        "session_id": self.session_id or "",
        "profile_id": profile_name_for_home(get_hermes_home()) or "default",
        "api_request_id": getattr(self, "_current_api_request_id", "") or "",
        "model": self.model or "",
        "provider": self.provider or "",
        "surface": self.platform or "cli",
    }
```

Then call the transform synchronously before delivering text, reasoning, and completed interim commentary:

```python
text = apply_llm_stream_text_middleware(
    text,
    kind="text",
    **self._stream_hook_base_payload(),
)
```

Use `kind="reasoning"` and `kind="interim"` on those paths. Returning `""` is valid so a privacy middleware may buffer a partial protected token.

- [ ] **Step 7: Run streaming regression tests**

Run:

```bash
pytest tests/agent/test_plugin_stream_hooks.py tests/agent/test_streaming.py tests/agent/test_stream_single_writer.py -v
```

Expected: PASS. Existing asynchronous observer hooks remain observers; the new synchronous middleware is the only transform path.

- [ ] **Step 8: Document and submit the upstream change**

Document that closed middleware is for security boundaries, and that `llm_stream_text` is synchronous because transformed bytes must precede display/TTS. Update this repository's Issue #1 to note both required upstream capabilities and record the upstream PR URL in `docs/upstream-hermes.md`.

- [ ] **Step 9: Commit upstream and documentation changes**

In the Hermes worktree:

```bash
git add hermes_cli/plugins.py hermes_cli/plugins_dispatch.py hermes_cli/middleware.py agent/stream_delivery.py tests/hermes_cli/test_plugins.py tests/agent/test_plugin_stream_hooks.py website/docs/developer-guide/middleware.md website/docs/developer-guide/plugins/index.md
git commit -m "feat: add fail-closed and stream text middleware"
```

In this repository:

```bash
git add docs/upstream-hermes.md
git commit -m "docs: track required Hermes privacy middleware"
```

---

### Task 2: Root Hermes plugin scaffold, configuration, and provider trust policy

**Files:**
- Create: `plugin.yaml`
- Create: `__init__.py`
- Create: `gaze_privacy/__init__.py`
- Create: `gaze_privacy/errors.py`
- Create: `gaze_privacy/config.py`
- Create: `gaze_privacy/provider_policy.py`
- Create: `tests/python/test_config.py`
- Create: `tests/python/test_provider_policy.py`
- Create: `pyproject.toml`

**Interfaces:**
- Consumes: Hermes plugin loader and host configuration/environment.
- Produces: `PrivacyConfig.load() -> PrivacyConfig`; `ProviderPolicy.classify(provider_id: str) -> ProtectionDecision`; `PrivacyBlockedError`.

- [ ] **Step 1: Write configuration and trust-policy tests**

```python
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
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pytest tests/python/test_config.py tests/python/test_provider_policy.py -v
```

Expected: FAIL because the package does not exist.

- [ ] **Step 3: Add plugin metadata and typed configuration**

Use a root plugin layout because Hermes hybrid-repo detection requires root `plugin.yaml` + `__init__.py`, with Desktop at `desktop/plugin.js`.

Core types:

```python
from dataclasses import dataclass
from enum import StrEnum

class ProtectionDecision(StrEnum):
    PROTECT = "protect"
    BYPASS = "bypass"

@dataclass(frozen=True)
class PrivacyConfig:
    home: Path
    sidecar_mode: str
    sidecar_scope: str
    sidecar_url: str
    trusted_local_providers: frozenset[str]
    mandatory_mode: bool
    compatibility_mode: bool
    api_token_file: Path
    master_key_file: Path
    global_policy_file: Path
    profile_policy_dir: Path

    @classmethod
    def load(cls) -> "PrivacyConfig":
        from hermes_constants import get_default_hermes_root

        override = os.getenv("GAZE_HERMES_HOME", "").strip()
        home = (
            Path(override).expanduser()
            if override
            else Path(get_default_hermes_root()) / "gaze-hermes-privacy"
        )
        raw = tomllib.loads((home / "config.toml").read_text("utf-8")) if (home / "config.toml").exists() else {}
        sidecar = raw.get("sidecar", {})
        providers = raw.get("providers", {})
        security = raw.get("security", {})
        scope = str(sidecar.get("scope", "host"))
        if scope not in {"host", "profile"}:
            raise ValueError("sidecar.scope must be 'host' or 'profile'")
        mode = str(sidecar.get("mode", "native"))
        if mode not in {"native", "docker", "external"}:
            raise ValueError("sidecar.mode must be native, docker, or external")
        return cls(
            home=home,
            sidecar_mode=mode,
            sidecar_scope=scope,
            sidecar_url=str(sidecar.get("url", "http://127.0.0.1:65113")),
            trusted_local_providers=frozenset(map(str, providers.get("trusted_local", []))),
            mandatory_mode=bool(security.get("mandatory", True)),
            compatibility_mode=bool(security.get("compatibility_mode", False)),
            api_token_file=Path(sidecar.get("api_token_file", home / "secrets" / "api-token")),
            master_key_file=Path(sidecar.get("master_key_file", home / "secrets" / "snapshot-key")),
            global_policy_file=Path(raw.get("policy", {}).get("global_file", home / "policies" / "global.toml")),
            profile_policy_dir=Path(raw.get("policy", {}).get("profile_dir", home / "policies" / "profiles")),
        )
```

The implementation must use `GAZE_HERMES_HOME` when set; otherwise resolve `hermes_constants.get_default_hermes_root()` and append `gaze-hermes-privacy`, so host-scoped state is not accidentally placed inside whichever profile happens to load the plugin first. Default `sidecar_mode` is `"native"`, default `sidecar_scope` is `"host"`, default URL is `http://127.0.0.1:65113`, `mandatory_mode` defaults true, and `compatibility_mode` defaults false.

- [ ] **Step 4: Implement exact-match provider policy**

```python
class ProviderPolicy:
    def __init__(self, trusted_local_providers: frozenset[str]):
        self._trusted = trusted_local_providers

    def classify(self, provider_id: str) -> ProtectionDecision:
        return (
            ProtectionDecision.BYPASS
            if provider_id in self._trusted
            else ProtectionDecision.PROTECT
        )
```

Do not inspect URL, hostname, provider display label, or process location.

- [ ] **Step 5: Run tests**

Run:

```bash
pytest tests/python/test_config.py tests/python/test_provider_policy.py -v
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add plugin.yaml __init__.py gaze_privacy pyproject.toml tests/python
git commit -m "feat: scaffold Hermes privacy plugin"
```

---

### Task 3: Rust sidecar process, authenticated loopback API, and protocol handshake

**Files:**
- Create: `sidecar/Cargo.toml`
- Create: `sidecar/Cargo.lock`
- Create: `sidecar/src/main.rs`
- Create: `sidecar/src/config.rs`
- Create: `sidecar/src/auth.rs`
- Create: `sidecar/src/api/mod.rs`
- Create: `sidecar/src/api/status.rs`
- Create: `sidecar/src/protocol.rs`
- Create: `sidecar/tests/health_auth.rs`

**Interfaces:**
- Consumes: API token file and process configuration.
- Produces: sidecar process on `127.0.0.1:65113` by default; optional `127.0.0.1:0` dynamic bind plus `--ready-file` rendezvous for per-profile mode; `GET /healthz`; authenticated `GET /v1/status`; protocol version `1`.

- [ ] **Step 1: Write failing router/auth tests**

```rust
#[tokio::test]
async fn status_requires_bearer_token() {
    let app = test_app("secret-token").await;
    let response = app.oneshot(
        Request::builder().uri("/v1/status").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn health_is_minimal_and_protocol_is_explicit() {
    let app = test_app("secret-token").await;
    let response = app.oneshot(
        Request::builder().uri("/healthz").body(Body::empty()).unwrap()
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["protocol_version"], 1);
    assert!(body.get("sessions").is_none());
}
```

- [ ] **Step 2: Add pinned Rust dependencies**

`sidecar/Cargo.toml` must set `rust-version = "1.89"` and include the dependencies below. Run `cargo generate-lockfile --manifest-path sidecar/Cargo.toml` after dependency resolution and commit `sidecar/Cargo.lock`; every later CI/Docker/release Cargo command uses `--locked`.

```toml
[dependencies]
axum = { version = "0.8", features = ["ws"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread", "net", "signal", "sync", "fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
clap = { version = "4", features = ["derive", "env"] }
thiserror = "2"
subtle = "2.6"
tracing = "0.1"
tracing-subscriber = "0.3"
```

- [ ] **Step 3: Run tests and verify failure**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test health_auth
```

Expected: FAIL because router/config modules are absent.

- [ ] **Step 4: Implement config, constant-time bearer auth, status, and readiness rendezvous**

Use `127.0.0.1:65113` as the default bind and permit `127.0.0.1:0` only when the manager explicitly requests an ephemeral loopback port for profile-isolated mode. Read the bearer token from `--api-token-file`; reject an empty/missing file. Compare equal-length byte strings using `subtle::ConstantTimeEq`.

If `--ready-file <path>` is supplied, bind the listener first, obtain `listener.local_addr()`, then atomically write a non-sensitive JSON rendezvous file:

```json
{"address":"127.0.0.1:43127","protocol_version":1}
```

The manager must never infer readiness from process existence alone.

Status contract:

```rust
#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub protocol_version: u32,
    pub sidecar_version: &'static str,
}
pub const PROTOCOL_VERSION: u32 = 1;
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test health_auth
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add sidecar
git commit -m "feat: add authenticated Gaze sidecar shell"
```

---

### Task 4: Gaze policy engine, profile overrides, and verified NER provisioning

**Files:**
- Modify: `sidecar/Cargo.toml`
- Create: `sidecar/src/policies/mod.rs`
- Create: `sidecar/src/policies/merge.rs`
- Create: `sidecar/src/policies/editor.rs`
- Create: `sidecar/src/model.rs`
- Create: `sidecar/tests/policies.rs`
- Create: `policies/default.toml`
- Create: `policies/strict.toml`
- Create: `policies/examples/custom-identifiers.toml`
- Create: `docs/policies.md`

**Interfaces:**
- Consumes: full global Gaze TOML policy plus optional profile override TOML.
- Produces: `PolicyStore::effective(profile_id) -> EffectivePolicy`; `PolicyStore::validate(raw_toml) -> ValidationResult`; `PolicyStore::edit(scope, expected_hash, PolicyEdit) -> EditedPolicy`; `PolicyStore::apply(scope, expected_hash, raw_toml) -> EffectivePolicy`; `ModelProvisioner::ensure() -> PathBuf`.

- [ ] **Step 1: Add exact Gaze dependencies**

```toml
gaze = { package = "gaze-pii", version = "=0.14.0" }
gaze-assembly = "=0.14.0"
gaze-recognizers = "=0.14.0"
gaze-model-setup = "=0.14.0"
toml = "0.9"
toml_edit = "0.23"
sha2 = "0.10"
```

- [ ] **Step 2: Write policy merge and validation tests**

Define profile override identity rules explicitly:

- recognisers are keyed by `name`;
- class rules are keyed by `class:<class>`;
- column rules are keyed by `column:<column>`;
- profile replacements override matching global entries;
- profile `remove_recognizers` and `remove_rules` delete inherited entries;
- unrelated/advanced global TOML is preserved.

Example test:

```rust
#[test]
fn profile_override_replaces_one_rule_without_flattening_base() {
    let base = r#"
schema_version = "0.1.0"
[[rule]]
kind = "class"
class = "email"
action = "tokenize"
[[rule]]
kind = "class"
class = "name"
action = "tokenize"
"#;
    let overlay = r#"
schema_version = "gaze-hermes-profile-1"
[[overrides.rules]]
kind = "class"
class = "email"
action = "redact"
"#;

    let effective = merge_policy_documents(base, overlay).unwrap();
    assert!(effective.contains("class = \"email\""));
    assert!(effective.contains("action = \"redact\""));
    assert!(effective.contains("class = \"name\""));
}
```

- [ ] **Step 3: Run tests and verify failure**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test policies
```

Expected: FAIL because policy modules are absent.

- [ ] **Step 4: Implement effective-policy assembly**

Load effective TOML into `gaze::Policy`, load embedded rulepacks requested by policy, construct `gaze::Context`, resolve `LocaleChain`, and call the current Gaze assembly API:

```rust
let policy = gaze::Policy::load(&effective_path)?;
let context = gaze::Context {
    dictionaries: HashMap::new(),
    class_map: HashMap::new(),
    fields: Default::default(),
};
let pipeline = gaze_assembly::build_pipeline(
    &policy,
    &context,
    &rulepacks,
    &active_locales,
    None,
)?;
```

Production code must build from the merged effective document, not from a hard-coded fallback pipeline.

- [ ] **Step 5: Implement a preserving visual-editor patch layer**

Use `toml_edit::DocumentMut` so visual operations edit only supported recogniser/rule nodes. Unknown keys and advanced-only nodes remain byte-preserving where `toml_edit` permits.

Expose internal operations:

```rust
pub enum PolicyEdit {
    UpsertRule(RuleEdit),
    RemoveRule { identity: String },
    UpsertRecognizer(RecognizerEdit),
    RemoveRecognizer { name: String },
}

pub enum PolicyScope {
    Global,
    Profile(String),
}
```

Policy changes use optimistic concurrency and atomic activation. `edit` returns a candidate TOML document without changing the active policy. `apply` checks `expected_hash`, parses and builds the candidate pipeline first, then atomically writes the canonical TOML and swaps the in-memory effective policy only after every validation/build step succeeds:

```rust
pub fn apply(
    &self,
    scope: PolicyScope,
    expected_hash: &str,
    raw_toml: &str,
) -> Result<EffectivePolicy, PolicyStoreError> {
    let current = self.document_for(&scope)?;
    if sha256_hex(current.as_bytes()) != expected_hash {
        return Err(PolicyStoreError::Conflict);
    }
    let candidate = self.build_candidate(&scope, raw_toml)?;
    atomic_write(self.path_for(&scope), raw_toml.as_bytes())?;
    self.install_candidate(scope, candidate.clone())?;
    Ok(candidate)
}
```

Tests must prove a bad candidate or stale `expected_hash` leaves both the file and active pipeline unchanged.

- [ ] **Step 6: Implement NER model provisioning behind a trait**

Production implementation calls Gaze's pinned installer:

```rust
pub trait ModelProvisioner: Send + Sync {
    fn ensure(&self) -> Result<PathBuf, ModelError>;
}

pub struct GazeModelProvisioner {
    pub model_dir: Option<PathBuf>,
}

impl ModelProvisioner for GazeModelProvisioner {
    fn ensure(&self) -> Result<PathBuf, ModelError> {
        use gaze_model_setup::InstallOutcome;
        match gaze_model_setup::install_ner_bundle(self.model_dir.as_deref())? {
            InstallOutcome::AlreadyPresent { model_dir }
            | InstallOutcome::Installed { model_dir } => Ok(model_dir),
        }
    }
}
```

Unit tests use a fake provisioner and never perform network downloads.

- [ ] **Step 7: Add open-source default policies**

`policies/default.toml` uses the portable bundled `core` rulepack and reversible actions:

```toml
schema_version = "0.1.0"

[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core"]

[[rule]]
kind = "class"
class = "email"
action = "tokenize"

[[rule]]
kind = "class"
class = "name"
action = "tokenize"

[[rule]]
kind = "class"
class = "location"
action = "tokenize"

[[rule]]
kind = "class"
class = "organization"
action = "tokenize"
```

`policies/strict.toml` uses `core-extended` and a protective restorable default:

```toml
schema_version = "0.1.0"

[session]
scope = "conversation"

[policy.rulepacks]
bundled = ["core-extended"]

[[rule]]
kind = "default"
action = "tokenize"
```

Neither file contains organisation-specific recognisers. NER remains enabled only when the effective policy has a verified model directory; first-run provisioning supplies that path through the profile/global policy setup flow rather than embedding a machine-specific path in the repository.

- [ ] **Step 8: Run tests**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test policies
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add sidecar/Cargo.toml sidecar/src/policies sidecar/src/model.rs sidecar/tests/policies.rs policies docs/policies.md
git commit -m "feat: add Gaze policy and model runtime"
```

---

### Task 5: Session registry and encrypted atomic snapshots

**Files:**
- Modify: `sidecar/Cargo.toml`
- Create: `sidecar/src/sessions/mod.rs`
- Create: `sidecar/src/sessions/store.rs`
- Create: `sidecar/src/crypto.rs`
- Create: `sidecar/tests/session_store.rs`

**Interfaces:**
- Consumes: `SessionKey { profile_id, session_id }`, master-key file, Gaze `Session::export/import`.
- Produces: `SessionRegistry::get_or_restore(&SessionKey) -> Arc<SessionHandle>`; `SessionRegistry::persist(&SessionKey)`; `SessionRegistry::delete(&SessionKey)`. `request_id` is deliberately not part of the persisted session key.

- [ ] **Step 1: Add encryption dependencies and write failure tests**

```toml
chacha20poly1305 = "0.10"
rand = "0.9"
hex = "0.4"
zeroize = "1.8"
```

Tests must cover:
- round-trip restore;
- wrong key;
- ciphertext tamper;
- stale `.tmp` file next to a valid snapshot;
- simulated failure before rename;
- profile/session namespace separation.

```rust
#[test]
fn tampered_snapshot_never_replaces_live_session() {
    let fixture = SnapshotFixture::new();
    fixture.persist("profile-a", "session-1", "alice@example.invalid");
    fixture.flip_ciphertext_byte("profile-a", "session-1");
    let err = fixture.restore("profile-a", "session-1").unwrap_err();
    assert!(matches!(err, StoreError::Decrypt | StoreError::Integrity));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test session_store
```

Expected: FAIL because registry/store do not exist.

- [ ] **Step 3: Implement snapshot envelope based on Gaze's proven pattern**

Use the same envelope pattern already exercised by Gaze's `gaze-mcp-bridge` file session store, adapted to a two-part namespace:
- ChaCha20-Poly1305;
- random 12-byte nonce;
- AAD equal to `b"gaze-hermes-session-v1\0" + profile_id + b"\0" + session_id`;
- a fixed magic/version header;
- SHA-256 of the same canonical profile/session bytes for the filename;
- `Session::export().into_bytes()` as plaintext before encryption;
- `Session::import(SensitiveSnapshot::from(plaintext))` after decryption.

Never include `request_id` in the filename, AAD, or Gaze conversation scope. Never write plaintext snapshot bytes to disk.

```rust
const MAGIC: &[u8] = b"gaze-hermes-session-v1\n";
const NONCE_LEN: usize = 12;

fn namespace_bytes(key: &SessionKey) -> Vec<u8> {
    let mut out = b"gaze-hermes-session-v1\0".to_vec();
    out.extend_from_slice(key.profile_id.as_bytes());
    out.push(0);
    out.extend_from_slice(key.session_id.as_bytes());
    out
}

fn encrypt_snapshot(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, StoreError> {
    let cipher = ChaCha20Poly1305::new(key.into());
    let mut nonce_bytes = [0_u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher.encrypt(
        Nonce::from_slice(&nonce_bytes),
        Payload { msg: plaintext, aad },
    ).map_err(|_| StoreError::Encrypt)?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}
```

- [ ] **Step 4: Implement atomic persistence**

Write to a sibling temporary file, `sync_all()`, then atomic rename. A stale temp file is ignored on read and may be cleaned on startup. The existing `.enc` remains authoritative until rename succeeds.

```rust
async fn persist_bytes(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    use tokio::io::AsyncWriteExt;
    let tmp = path.with_extension(format!("tmp-{}", random_hex(8)));
    let mut file = tokio::fs::File::create(&tmp).await?;
    file.write_all(bytes).await?;
    file.sync_all().await?;
    drop(file);
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}
```

- [ ] **Step 5: Implement recovery refusal**

If an encrypted snapshot exists but cannot decrypt/import, return a typed recovery error. Do not create a new session with the same namespace until the operator explicitly resets/deletes the broken snapshot.

```rust
match tokio::fs::read(&path).await {
    Ok(ciphertext) => restore_snapshot(&master_key, &session_key, &ciphertext)
        .map_err(StoreError::RecoveryBlocked),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
        gaze::Session::new(gaze::Scope::Conversation(session_key.session_id.clone()))
            .map_err(StoreError::Session)
    }
    Err(err) => Err(StoreError::Io(err)),
}
```

`RecoveryBlocked` is surfaced by the API/UI until an explicit delete/reset operation removes the corrupt snapshot.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test session_store
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add sidecar/Cargo.toml sidecar/src/sessions sidecar/src/crypto.rs sidecar/tests/session_store.rs
git commit -m "feat: persist encrypted Gaze sessions"
```

---

### Task 6: Transactional privacy and management REST API

**Files:**
- Create: `sidecar/src/api/privacy.rs`
- Create: `sidecar/src/api/policies.rs`
- Create: `sidecar/src/api/sessions.rs`
- Create: `sidecar/src/api/metrics.rs`
- Modify: `sidecar/src/api/mod.rs`
- Modify: `sidecar/src/sessions/mod.rs`
- Create: `sidecar/tests/privacy_api.rs`
- Create: `sidecar/tests/management_api.rs`

**Interfaces:**
- Consumes: `PolicyStore`, `SessionRegistry`.
- Produces:
  - `POST /v1/clean`
  - `POST /v1/restore`
  - `POST /v1/policies/validate`
  - `POST /v1/policies/test`
  - `POST /v1/policies/edit`
  - `POST /v1/policies/apply`
  - `GET /v1/policies/effective`
  - `GET /v1/sessions`
  - `GET /v1/sessions/{profile_id}/{session_id}`
  - `POST /v1/sessions/{profile_id}/{session_id}/recover`
  - `DELETE /v1/sessions/{profile_id}/{session_id}`
  - `GET /v1/metrics`
  - canonical `SessionKey`, `RequestNamespace`, `TextField`, `CleanRequest/Response`, `RestoreRequest/Response`.

- [ ] **Step 1: Define canonical field protocol and write failing tests**

```rust
#[derive(Serialize, Deserialize, Clone, Eq, PartialEq, Hash)]
pub struct SessionKey {
    pub profile_id: String,
    pub session_id: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RequestNamespace {
    pub profile_id: String,
    pub session_id: String,
    pub request_id: String,
}

impl RequestNamespace {
    pub fn session_key(&self) -> SessionKey {
        SessionKey {
            profile_id: self.profile_id.clone(),
            session_id: self.session_id.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TextField {
    pub path: String,
    pub text: String,
}

#[derive(Serialize, Deserialize)]
pub struct CleanRequest {
    pub namespace: RequestNamespace,
    pub fields: Vec<TextField>,
}
```

A multi-field clean must be atomic: if field 3 fails, mappings staged by fields 1 and 2 are not committed.

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test privacy_api
```

Expected: FAIL because the routes are absent.

- [ ] **Step 3: Implement one Gaze transaction across all outbound fields**

```rust
let session_key = request.namespace.session_key();
let handle = registry.get_or_restore(&session_key).await?;
let mut session = handle.session.lock().await;
let mut tx = session.begin_transaction();
let mut cleaned = Vec::with_capacity(request.fields.len());

for field in &request.fields {
    let text = pipeline.protect_text_transaction(
        &mut tx,
        &field.text,
        protection_context,
    )?;
    cleaned.push(TextField { path: field.path.clone(), text });
}

tx.commit()?;
drop(session);
registry.persist(&session_key).await?;
```

Only commit after all fields succeed.

- [ ] **Step 4: Implement strict restore**

Restore every field with `Session::restore_strict_text`. Unknown/malformed owned-token syntax becomes a typed 422 privacy error, never a pass-through success.

```rust
let session_key = request.namespace.session_key();
let handle = registry.get_or_restore(&session_key).await?;
let session = handle.session.lock().await;
let mut restored = Vec::with_capacity(request.fields.len());
for field in &request.fields {
    let text = session
        .restore_strict_text(&field.text)
        .map_err(PrivacyApiError::StrictRestore)?;
    restored.push(TextField { path: field.path.clone(), text });
}
```

Map `PrivacyApiError::StrictRestore` to HTTP 422 with a sanitised code such as `strict_restore_failed`; never include the raw text in the error body.

- [ ] **Step 5: Return sanitised detection summaries**

Responses may include class/count metadata but never raw mappings:

```json
{
  "fields": [{"path": "/messages/0/content", "text": "Contact <token>"}],
  "detections": [{"class": "email", "count": 1}],
  "policy_version": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
}
```

- [ ] **Step 6: Expose authenticated policy/session management routes**

All routes in this step use the bearer middleware from Task 3. Policy test operations use a temporary Gaze session and never modify the active policy or contact an LLM provider.

```rust
async fn validate_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyDocumentRequest>,
) -> Result<Json<ValidationResponse>, ApiError> {
    Ok(Json(state.policies.validate(&request.toml)?))
}

async fn apply_policy(
    State(state): State<AppState>,
    Json(request): Json<PolicyApplyRequest>,
) -> Result<Json<EffectivePolicyResponse>, ApiError> {
    let effective = state.policies.apply(
        request.scope,
        &request.expected_hash,
        &request.toml,
    )?;
    Ok(Json(EffectivePolicyResponse::from(effective)))
}

async fn delete_session(
    State(state): State<AppState>,
    Path((profile_id, session_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    state.sessions.delete(&SessionKey { profile_id, session_id }).await?;
    Ok(StatusCode::NO_CONTENT)
}
```

`GET /v1/sessions` and the per-session GET return only namespace, timestamps, snapshot/recovery state, mapping count, and policy hash. They never return token-to-raw mappings. `POST /recover` calls `get_or_restore` and reports success/error without exporting mappings. `GET /v1/metrics` exposes aggregate counters/latency only.

Add management API tests proving:
- every management route except `/healthz` returns 401 without bearer auth;
- invalid policy apply leaves the active hash unchanged;
- stale `expected_hash` returns 409;
- policy test round-trips synthetic text without changing the active policy;
- session list/get never contains raw values or tokens;
- deleting a session removes its encrypted snapshot.

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test privacy_api --test management_api
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add sidecar/src/api sidecar/src/sessions/mod.rs sidecar/tests/privacy_api.rs sidecar/tests/management_api.rs
git commit -m "feat: add transactional privacy management API"
```

---

### Task 7: Stateful WebSocket stream restoration

**Files:**
- Create: `sidecar/src/streaming/mod.rs`
- Create: `sidecar/src/streaming/restorer.rs`
- Create: `sidecar/src/api/streams.rs`
- Modify: `sidecar/src/api/mod.rs`
- Create: `sidecar/tests/streaming.rs`

**Interfaces:**
- Consumes: authenticated WebSocket, session namespace, Gaze strict restore.
- Produces: `WS /v1/streams/{stream_id}` with `open/chunk/finish/abort`; per-lane carry buffer; strict global sequence checking.

- [ ] **Step 1: Write state-machine tests for split tokens and lanes**

Use a session that maps one real value to one actual Gaze token generated by Task 6; do not hard-code a guessed token grammar.

Test:
- token split at every byte boundary;
- two tokens in one chunk;
- text/reasoning lane interleaving;
- duplicate sequence;
- skipped sequence;
- finish with incomplete reserved token prefix;
- abort removes stream state.

```rust
#[test]
fn split_token_is_never_emitted_partially() {
    let (session, token) = fixture_session_with_email("alice@example.invalid");
    for split in 1..token.len() {
        let mut r = StreamRestorer::new(session.clone());
        assert_eq!(r.feed(1, "text", &token[..split]).unwrap(), "");
        assert_eq!(
            r.feed(2, "text", &token[split..]).unwrap(),
            "alice@example.invalid"
        );
    }
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test streaming
```

Expected: FAIL because the stream restorer does not exist.

- [ ] **Step 3: Implement per-lane carry buffering without copying Gaze's token grammar**

Maintain:
- one monotonically increasing message sequence for the WebSocket;
- independent carry buffers keyed by `kind` (`text`, `reasoning`, `interim`);
- the active session's exact `Session::tokens()` strings captured after request cleaning.

Do not hard-code Gaze's token grammar. Retain the longest suffix of `carry + chunk` that is a proper prefix of any token actually owned by this session. Everything before that suffix is safe to pass immediately through `restore_strict_text`.

```rust
fn longest_owned_token_prefix_suffix(text: &str, tokens: &[String]) -> usize {
    let mut starts: Vec<usize> = text.char_indices().map(|(index, _)| index).collect();
    starts.push(text.len());
    for start in starts.into_iter().rev() {
        let suffix = &text[start..];
        if !suffix.is_empty()
            && tokens.iter().any(|token| token.starts_with(suffix) && token.len() > suffix.len())
        {
            return suffix.len();
        }
    }
    0
}

fn feed_lane(
    session: &gaze::Session,
    tokens: &[String],
    carry: &mut String,
    chunk: &str,
) -> Result<String, StreamError> {
    carry.push_str(chunk);
    let held = longest_owned_token_prefix_suffix(carry, tokens);
    let safe_len = carry.len() - held;
    let safe = carry[..safe_len].to_string();
    let pending = carry[safe_len..].to_string();
    *carry = pending;
    session.restore_strict_text(&safe).map_err(StreamError::StrictRestore)
}
```

On `finish`, strict-restore every remaining lane carry and require success before closing. An invented/unknown token-shaped value therefore reaches Gaze's strict validator and fails closed rather than being emitted. The restorer may return an empty string while it holds a possible owned-token prefix, and lane carries are never concatenated.

- [ ] **Step 4: Implement strict WebSocket protocol**

Messages:

```json
{"type":"open","namespace":{"profile_id":"default","session_id":"s1","request_id":"r1"}}
{"type":"chunk","seq":1,"kind":"text","text":"Hello <partial"}
{"type":"chunk","seq":2,"kind":"text","text":" token>"}
{"type":"finish"}
```

Server replies to each chunk with the same sequence and restored text. `finish` succeeds only when every lane buffer is empty/non-token text; a dangling protected-token prefix fails the stream.

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test --manifest-path sidecar/Cargo.toml --test streaming
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add sidecar/src/streaming sidecar/src/api/streams.rs sidecar/src/api/mod.rs sidecar/tests/streaming.rs
git commit -m "feat: restore protected streams over WebSocket"
```

---

### Task 8: Python sidecar client, secure bootstrap, and native-first lifecycle manager

**Files:**
- Create: `gaze_privacy/sidecar_client.py`
- Create: `gaze_privacy/sidecar_manager.py`
- Create: `gaze_privacy/secrets.py`
- Create: `gaze_privacy/release_manifest.py`
- Create: `sidecar-release.json`
- Create: `tests/python/test_sidecar_client.py`
- Create: `tests/python/test_sidecar_manager.py`
- Create: `tests/python/test_secrets.py`

**Interfaces:**
- Consumes: `PrivacyConfig`, active `profile_id`, release manifest, sidecar REST/WebSocket protocol.
- Produces: `SidecarManager.ensure_running(profile_id: str) -> ManagedSidecar`; `ManagedSidecar { status: SidecarStatus, client: SidecarClient }`; `SidecarClient.clean/restore/open_stream`; generated secret files with restrictive permissions. Host scope reuses one process/client; profile scope returns a profile-specific endpoint/process/client.

- [ ] **Step 1: Write secret/bootstrap tests**

Tests must prove:
- existing operator-supplied secret files are never overwritten;
- generated secrets are random and persistent across reloads;
- POSIX permissions are `0o600`;
- a binary with wrong SHA-256 is deleted/refused;
- Docker/external mode never attempts a native download;
- host scope reuses one compatible endpoint across profiles;
- profile scope launches separate endpoints and state directories and consumes the sidecar `--ready-file` result rather than guessing a port.

```python
def test_generated_secret_is_persistent_and_private(tmp_path):
    path = tmp_path / "api-token"
    first = ensure_secret(path)
    second = ensure_secret(path)
    assert first == second
    assert first
    if os.name != "nt":
        assert stat.S_IMODE(path.stat().st_mode) == 0o600


def test_profile_scope_uses_ready_file_endpoint(manager, fake_launcher):
    manager.config = replace(manager.config, sidecar_scope="profile")
    fake_launcher.ready_address = "127.0.0.1:43127"
    managed = manager.ensure_running("writer")
    assert managed.client.base_url == "http://127.0.0.1:43127"
    assert fake_launcher.argv_contains("--bind", "127.0.0.1:0")
```

Add adjacent tests for operator-supplied secrets, bad binary hashes, Docker/external no-download behavior, and host-scope endpoint reuse.

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
pytest tests/python/test_secrets.py tests/python/test_sidecar_manager.py -v
```

Expected: FAIL because lifecycle code is absent.

- [ ] **Step 3: Implement generated-secret fallback**

```python
def ensure_secret(path: Path, *, nbytes: int = 32) -> str:
    if path.exists():
        return path.read_text(encoding="utf-8").strip()
    path.parent.mkdir(parents=True, exist_ok=True)
    value = secrets.token_urlsafe(nbytes)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as handle:
        handle.write(value)
    return value
```

The master-key file uses random bytes encoded safely for the Rust reader and is distinct from the API token.

- [ ] **Step 4: Implement native artifact verification**

`sidecar-release.json` maps platform/architecture to URL, SHA-256, and sidecar protocol version. Download to a temporary file, hash it, set executable permissions only after the hash matches, then atomically rename into `bin/`.

Define:

```python
@dataclass(frozen=True)
class ManagedSidecar:
    status: SidecarStatus
    client: SidecarClient
```

`ensure_running(profile_id)` returns the client bound to the endpoint it actually verified. Callers never reconstruct an endpoint from config after startup.

- [ ] **Step 5: Implement native-first process management**

Order:
1. if `sidecar_mode=external`, health-check the configured endpoint selected for the current scope;
2. if `sidecar_mode=docker`, health-check the configured container endpoint and surface setup guidance if absent;
3. default `native` + host scope: reuse the healthy compatible host sidecar or launch the verified binary on `127.0.0.1:65113`;
4. default `native` + profile scope: launch/reuse a process under `run/<safe-profile-id>/`, pass `--bind 127.0.0.1:0 --ready-file <profile-run-dir>/ready.json`, and use the reported address;
5. poll `/healthz` until ready using a bounded startup timeout;
6. reject protocol mismatch.

Use a per-scope lock/rendezvous file so two Hermes processes racing to start the same sidecar do not spawn duplicates. Host scope may be shared by profiles; profile scope must never reuse another profile's process or snapshot directory.

```python
def ensure_running(self, profile_id: str) -> ManagedSidecar:
    scope = "host" if self.config.sidecar_scope == "host" else safe_profile_id(profile_id)
    with self._scope_lock(scope):
        endpoint = self._existing_healthy_endpoint(scope)
        if endpoint is None:
            endpoint = self._launch_or_resolve_endpoint(scope, profile_id)
        client = SidecarClient(endpoint, token=self._api_token())
        status = client.status()
        if status.protocol_version != SIDECAR_PROTOCOL_VERSION:
            raise PrivacyProtocolError("sidecar protocol mismatch")
        return ManagedSidecar(status=status, client=client)
```

- [ ] **Step 6: Implement REST and WebSocket client**

Use Hermes' existing `websockets` and `httpx` dependencies. `StreamClient.feed(kind, text) -> str` serialises sends under a lock and verifies returned sequence numbers.

```python
class SidecarClient:
    def __init__(self, base_url: str, token: str):
        self.base_url = base_url.rstrip("/")
        self.headers = {"Authorization": f"Bearer {token}"}

    def clean(self, namespace: dict, fields: list[dict]) -> dict:
        response = httpx.post(
            f"{self.base_url}/v1/clean",
            headers=self.headers,
            json={"namespace": namespace, "fields": fields},
            timeout=30.0,
        )
        response.raise_for_status()
        return response.json()

    def open_stream(self, namespace: dict) -> "StreamClient":
        stream_id = uuid.uuid4().hex
        ws_url = (
            self.base_url.replace("http://", "ws://", 1)
            + "/v1/streams/"
            + stream_id
        )
        return StreamClient.connect(ws_url, self.headers, namespace)
```

The real client uses the same bounded timeout and sanitised error translation for restore, policy, and session endpoints.

- [ ] **Step 7: Run tests**

Run:

```bash
pytest tests/python/test_secrets.py tests/python/test_sidecar_manager.py tests/python/test_sidecar_client.py -v
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add gaze_privacy sidecar-release.json tests/python
git commit -m "feat: manage native privacy sidecar"
```

---

### Task 9: Provider request/response adapters and mandatory opaque-field blocking

**Files:**
- Create: `gaze_privacy/adapters/__init__.py`
- Create: `gaze_privacy/adapters/common.py`
- Create: `gaze_privacy/adapters/chat.py`
- Create: `gaze_privacy/adapters/responses.py`
- Create: `gaze_privacy/adapters/bedrock.py`
- Create: `tests/python/test_request_adapters.py`
- Create: `tests/python/test_response_adapters.py`

**Interfaces:**
- Consumes: Hermes `api_mode`, provider kwargs, completed provider response object.
- Produces: `PreparedPayload(fields, apply)`; `extract_request_fields(api_mode, request)`; `extract_response_fields(api_mode, response)`; typed `UnsupportedCarrierError`.

- [ ] **Step 1: Write request extraction tests for every supported wire shape**

Protect text-bearing fields while preserving structural identifiers. Cover each wire family with a concrete fixture:

```python
@pytest.mark.parametrize(
    ("api_mode", "payload", "expected_paths"),
    [
        (
            "chat_completions",
            {"model": "m", "messages": [{"role": "user", "content": "Email alice@example.invalid"}]},
            ["/messages/0/content"],
        ),
        (
            "anthropic_messages",
            {"model": "m", "system": "Contact Alice", "messages": [{"role": "user", "content": [{"type": "text", "text": "York"}]}]},
            ["/system", "/messages/0/content/0/text"],
        ),
        (
            "codex_responses",
            {"model": "m", "instructions": "Contact Alice", "input": [{"role": "user", "content": [{"type": "input_text", "text": "York"}]}]},
            ["/instructions", "/input/0/content/0/text"],
        ),
        (
            "bedrock_converse",
            {"modelId": "m", "messages": [{"role": "user", "content": [{"text": "Email alice@example.invalid"}]}]},
            ["/messages/0/content/0/text"],
        ),
    ],
)
def test_supported_wire_extracts_only_text_fields(api_mode, payload, expected_paths):
    prepared = extract_request_fields(api_mode, payload, mandatory=True)
    assert [field.path for field in prepared.fields] == expected_paths
    assert prepared.payload.get("model", prepared.payload.get("modelId")) == "m"
```

Additional fixtures cover tool results, tool descriptions, and JSON-schema descriptions. Assert model IDs, roles, content-part `type`, function/tool names, and schema property keys are not modified.

- [ ] **Step 2: Add Review Focus test for opaque/multimodal content**

```python
@pytest.mark.parametrize(
    "payload",
    [
        {"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}]}]},
        {"input": [{"type": "input_audio", "audio": "opaque"}]},
        {"messages": [{"role": "user", "content": [{"type": "unknown_blob", "payload": "opaque"}]}]},
    ],
)
def test_mandatory_mode_rejects_uninspectable_carriers(payload):
    with pytest.raises(UnsupportedCarrierError):
        extract_request_fields("chat_completions", payload, mandatory=True)
```

Trusted-local bypass never calls these adapters, so local providers may still receive such payloads.

- [ ] **Step 3: Run tests and verify failure**

Run:

```bash
pytest tests/python/test_request_adapters.py tests/python/test_response_adapters.py -v
```

Expected: FAIL because adapters do not exist.

- [ ] **Step 4: Implement path-addressed field extraction**

Use JSON-pointer-like paths and copy-on-write application:

```python
@dataclass
class PreparedPayload:
    payload: Any
    fields: list[TextField]

    def apply(self, transformed: list[TextField]) -> Any:
        if [f.path for f in transformed] != [f.path for f in self.fields]:
            raise PrivacyProtocolError("sidecar returned mismatched field paths")
        result = copy.deepcopy(self.payload)
        for field in transformed:
            set_path(result, field.path, field.text)
        return result
```

Never recursively transform all strings blindly.

- [ ] **Step 5: Implement completed-response extraction**

For Chat/Anthropic/Bedrock-normalised responses restore `choices[].message.content`, reasoning/refusal text, and `tool_calls[].function.arguments`. For Responses/Codex restore message output/refusal/commentary text, function-call `arguments`, and `output_text`.

```python
def restore_completed_response(client, namespace, api_mode, response):
    prepared = extract_response_fields(api_mode, response)
    if not prepared.fields:
        return response
    restored = client.restore(namespace, prepared.fields)
    return prepared.apply(restored["fields"])
```

The response extractor must leave encrypted signatures, IDs, model names, tool names, and structural metadata untouched. Add a test where a `write_file` argument contains a Gaze token and assert only the argument string is restored.

- [ ] **Step 6: Run tests**

Run:

```bash
pytest tests/python/test_request_adapters.py tests/python/test_response_adapters.py -v
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add gaze_privacy/adapters tests/python/test_request_adapters.py tests/python/test_response_adapters.py
git commit -m "feat: adapt Hermes provider payloads for privacy"
```

---

### Task 10: Hermes middleware integration, streaming bridge, compatibility mode, and fallbacks

**Files:**
- Modify: `__init__.py`
- Create: `gaze_privacy/middleware.py`
- Create: `gaze_privacy/runtime.py`
- Create: `gaze_privacy/events.py`
- Create: `tests/python/test_middleware.py`
- Create: `tests/python/test_stream_bridge.py`

**Interfaces:**
- Consumes: Tasks 1, 2, 8, and 9.
- Produces: `PrivacyRuntime.execute(*, request: dict[str, Any], next_call: Callable[[dict[str, Any]], Any], provider: str, api_mode: str, **context: Any) -> Any`; closed `llm_execution` middleware; closed `llm_stream_text` middleware; thread-safe `StreamRegistry` keyed by `(profile_id, session_id, api_request_id)`; sanitised `PrivacyEvent`.

- [ ] **Step 1: Write protected-request lifecycle test**

```python
def test_external_provider_receives_only_cleaned_request(runtime):
    seen = {}

    def provider(request):
        seen["request"] = request
        return fake_chat_response("Hello <token>")

    result = runtime.execute(
        request={"messages": [{"role": "user", "content": "Email alice@example.invalid"}]},
        next_call=provider,
        provider="openrouter",
        api_mode="chat_completions",
        session_id="s1",
        turn_id="t1",
        api_request_id="t1:api:1",
    )

    assert "alice@example.invalid" not in repr(seen["request"])
    assert result.choices[0].message.content == "Hello alice@example.invalid"
```

- [ ] **Step 2: Add fallback trust regression test**

Simulate consecutive Hermes attempts and assert trust is re-evaluated every time:

```python
def test_fallback_rechecks_provider_trust(runtime):
    runtime.provider_policy = ProviderPolicy(frozenset({"local-vllm"}))
    calls = []

    runtime.execute(
        provider="local-vllm",
        next_call=lambda request: calls.append(("local", request)) or fake_response(),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    runtime.execute(
        provider="openrouter",
        next_call=lambda request: calls.append(("openrouter", request)) or fake_response(),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r2",
    )
    runtime.execute(
        provider="anthropic",
        next_call=lambda request: calls.append(("anthropic", request)) or fake_response(),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r3",
    )

    assert runtime.fake_sidecar.clean_calls == 2
    assert [name for name, _ in calls] == ["local", "openrouter", "anthropic"]
```

- [ ] **Step 3: Add mandatory compatibility tests**

Feature detection checks both `register_middleware` accepting `failure_mode` and Hermes accepting the `llm_stream_text` kind:

```python
def test_missing_hermes_privacy_capabilities_block_external(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.config = replace(runtime.config, mandatory_mode=True)
    with pytest.raises(PrivacyBlockedError):
        runtime.execute(
            provider="openrouter",
            next_call=lambda request: pytest.fail("provider must not be called"),
            request=payload(),
            api_mode="chat_completions",
            session_id="s1",
            api_request_id="r1",
        )


def test_missing_capabilities_still_allow_explicit_trusted_local(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.provider_policy = ProviderPolicy(frozenset({"local-vllm"}))
    before = runtime.fake_sidecar.clean_calls
    result = runtime.execute(
        provider="local-vllm",
        next_call=lambda request: fake_response(),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    assert result is not None
    assert runtime.fake_sidecar.clean_calls == before


def test_compatibility_mode_marks_external_call_not_guaranteed(runtime):
    runtime.capabilities = HermesCapabilities(fail_closed=False, stream_text=False)
    runtime.config = replace(runtime.config, compatibility_mode=True)
    runtime.execute(
        provider="openrouter",
        next_call=lambda request: fake_response(),
        request=payload(),
        api_mode="chat_completions",
        session_id="s1",
        api_request_id="r1",
    )
    assert runtime.events.last().state == "protection_not_guaranteed"
```

- [ ] **Step 4: Run tests and verify failure**

Run:

```bash
pytest tests/python/test_middleware.py tests/python/test_stream_bridge.py -v
```

Expected: FAIL because runtime middleware is absent.

- [ ] **Step 5: Implement `llm_execution` middleware**

Exact ordering:

```python
class PrivacyRuntime:
    def execute(self, *, request, next_call, provider, api_mode, **ctx):
        if self.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
            self.events.record_bypass(provider=provider, **ctx)
            return next_call(request)

        self.require_or_mark_capabilities(provider=provider, context=ctx)

        from hermes_constants import get_hermes_home, profile_name_for_home
        profile_id = profile_name_for_home(get_hermes_home()) or "default"
        managed = self.sidecars.ensure_running(profile_id)
        namespace = namespace_from(ctx, profile_id=profile_id)

        prepared = extract_request_fields(api_mode, request, mandatory=self.config.mandatory_mode)
        cleaned = managed.client.clean(namespace, prepared.fields)
        protected_request = prepared.apply(cleaned.fields)

        request_key = self.streams.reserve(namespace, client=managed.client)
        try:
            response = next_call(protected_request)
            restored = restore_completed_response(
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

def llm_execution_middleware(**kwargs):
    return runtime.execute(**kwargs)
```

If outbound cleaning fails, `next_call` is never invoked.

- [ ] **Step 6: Implement synchronous live-stream transform**

```python
def llm_stream_text_middleware(
    *,
    text,
    kind,
    provider,
    profile_id,
    session_id,
    api_request_id,
    **_ctx,
):
    if runtime.provider_policy.classify(provider) is ProtectionDecision.BYPASS:
        return {"text": text}
    runtime.require_or_mark_capabilities(provider=provider, context=_ctx)
    key = (
        str(profile_id or "default"),
        str(session_id or ""),
        str(api_request_id or ""),
    )
    stream = runtime.streams.get_or_open(key)
    return {"text": stream.feed(kind=kind, text=text)}
```

Implement capability gating once in `PrivacyRuntime`:

```python
def require_or_mark_capabilities(self, *, provider: str, context: dict) -> None:
    if self.capabilities.fail_closed and self.capabilities.stream_text:
        return
    if self.config.compatibility_mode:
        self.events.record(
            state="protection_not_guaranteed",
            provider=provider,
            reason="hermes_capability_missing",
            **context,
        )
        return
    raise PrivacyBlockedError("Hermes lacks required fail-closed privacy capabilities")
```

`StreamRegistry.reserve(namespace, client=managed.client)` runs before `next_call` and stores the exact profile-aware client selected by `SidecarManager` under `(profile_id, session_id, api_request_id)`. `get_or_open()` lazily opens that client's authenticated sidecar WebSocket on the first live delta and rejects an unknown request key. This avoids relying on implicit cross-profile uniqueness or a `ContextVar` crossing Hermes' streaming worker threads, and avoids opening a WebSocket for non-streaming calls. Trusted-local streams pass through unchanged.

Register both middleware callbacks with `failure_mode="closed"`.

- [ ] **Step 7: Ensure final response restoration remains separate from live display restoration**

Live stream restoration changes only what Hermes displays/TTS/interim-delivers. The completed response returned by `next_call` is independently restored through Task 9's response adapter before Hermes parses tool calls. This is what makes streamed `write_file` arguments contain real values at execution time without exposing tool JSON fragments to the UI.

- [ ] **Step 8: Run tests**

Run:

```bash
pytest tests/python/test_middleware.py tests/python/test_stream_bridge.py -v
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add __init__.py gaze_privacy/middleware.py gaze_privacy/runtime.py gaze_privacy/events.py tests/python/test_middleware.py tests/python/test_stream_bridge.py
git commit -m "feat: enforce Gaze around Hermes LLM calls"
```

---

### Task 11: Backend plugin API, sanitised event buffer, policy/session control, and sensitive reveal

**Files:**
- Create: `dashboard/manifest.json`
- Create: `dashboard/plugin_api.py`
- Create: `gaze_privacy/plugin_api_service.py`
- Create: `gaze_privacy/reveal.py`
- Create: `tests/python/test_plugin_api.py`
- Create: `tests/python/test_reveal.py`

**Interfaces:**
- Consumes: runtime, policy store via sidecar, sanitised event buffer.
- Produces: Desktop-safe REST namespace:
  - `GET /status`
  - `GET /events`
  - `WS /events`
  - `GET/PUT /policies/global`
  - `GET/PUT /policies/profiles/{profile_id}`
  - `POST /policies/validate`
  - `POST /policies/test`
  - `POST /policies/apply`
  - `POST /policies/edit`
  - `GET /providers`
  - `PUT /providers/{provider_id}/trust`
  - `GET /sessions`
  - `POST /sessions/{profile_id}/{session_id}/recover`
  - `DELETE /sessions/{profile_id}/{session_id}`
  - `POST /events/{event_id}/reveal`.

- [ ] **Step 1: Write API tests**

Use FastAPI `TestClient`. Assert status returns capability state and sidecar mode without secrets, and policy writes validate before atomic activation.

```python
def test_status_is_desktop_safe(client):
    response = client.get("/status")
    assert response.status_code == 200
    body = response.json()
    assert body["sidecar"]["mode"] in {"native", "docker", "external"}
    serialized = json.dumps(body)
    assert "api_token" not in serialized
    assert "master_key" not in serialized
    assert "Authorization" not in serialized


def test_invalid_policy_never_replaces_active_policy(client, active_policy_text):
    response = client.post("/policies/apply", json={"scope": "global", "toml": "not = [valid"})
    assert response.status_code == 422
    assert client.get("/policies/global").json()["toml"] == active_policy_text
```

- [ ] **Step 2: Add Review Focus tests for reveal behaviour**

Backend reveal tokens are single-event, short-lived capabilities:

```python
def test_reveal_token_expires_and_never_enters_event_log(service, clock):
    event_id = service.events.add_sanitised(sample_event())
    grant = service.reveal.issue(event_id, ttl_seconds=60)
    assert service.reveal.read(grant.token)["original"] == "synthetic@example.invalid"

    clock.advance(61)
    with pytest.raises(RevealExpired):
        service.reveal.read(grant.token)

    assert "synthetic@example.invalid" not in service.events.serialized_log()
```

Also test profile mismatch and reuse after successful read if grants are single-use.

- [ ] **Step 3: Run tests and verify failure**

Run:

```bash
pytest tests/python/test_plugin_api.py tests/python/test_reveal.py -v
```

Expected: FAIL because the API service does not exist.

- [ ] **Step 4: Implement narrow Desktop-facing service**

`dashboard/plugin_api.py` is a thin `APIRouter` wrapper. Keep sidecar credentials inside `gaze_privacy/plugin_api_service.py`; never return them.

```python
router = APIRouter()
service = PluginApiService.from_runtime()

@router.get("/status")
def status():
    return service.status()

@router.get("/events")
def events(limit: int = 100):
    return {"events": service.events.list(limit=min(max(limit, 1), 500))}

@router.websocket("/events")
async def events_socket(websocket: WebSocket):
    if not ws_upgrade_authorized(websocket):
        await websocket.close(code=4401)
        return
    await websocket.accept()
    subscription = service.events.subscribe()
    try:
        async for event in subscription:
            await websocket.send_json(event.sanitised_dict())
    finally:
        subscription.close()

@router.post("/policies/validate")
def validate_policy(request: PolicyTextRequest):
    return service.validate_policy(request)
```

Every route delegates to typed service methods. `ws_upgrade_authorized` delegates to Hermes' canonical dashboard WebSocket auth gate, matching the built-in plugin pattern. Router exceptions and WebSocket error frames contain codes/metadata only, never sensitive request bodies. Add a TestClient WebSocket test that receives one event and asserts synthetic PII is absent.

- [ ] **Step 5: Implement bounded sanitised event storage**

Use a fixed-size deque. Persist only metadata permitted by the spec. Sensitive original/protected/restored payloads, when debug capture is enabled, stay in an in-memory ephemeral store keyed by event ID and are cleared on session end/restart.

```python
class EventBuffer:
    def __init__(self, max_events: int = 500):
        self._events = deque(maxlen=max_events)
        self._sensitive: dict[str, dict[str, str]] = {}

    def add(self, event: PrivacyEvent, sensitive: dict[str, str] | None = None) -> None:
        self._events.append(event.sanitised_dict())
        if sensitive is not None:
            self._sensitive[event.id] = dict(sensitive)

    def clear_session(self, profile_id: str, session_id: str) -> None:
        self._sensitive = {
            event_id: value
            for event_id, value in self._sensitive.items()
            if not value_matches_session(value, profile_id, session_id)
        }
```

- [ ] **Step 6: Implement temporary reveal grants**

Generate random opaque grants, bind them to event/profile, expire after 60 seconds, and never persist grants or returned data. A plugin setting may shorten the timeout but not disable expiration.

```python
@dataclass
class RevealGrant:
    event_id: str
    profile_id: str
    expires_at: float
    consumed: bool = False

def issue_reveal(self, event_id: str, profile_id: str, ttl_seconds: int = 60) -> str:
    ttl = min(max(int(ttl_seconds), 1), 60)
    token = secrets.token_urlsafe(32)
    self._grants[token] = RevealGrant(
        event_id=event_id,
        profile_id=profile_id,
        expires_at=self._clock() + ttl,
    )
    return token
```

`read_reveal(token, profile_id)` rejects missing, expired, profile-mismatched, or consumed grants; successful reads mark the grant consumed before returning the in-memory payload.

- [ ] **Step 7: Run tests**

Run:

```bash
pytest tests/python/test_plugin_api.py tests/python/test_reveal.py -v
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add dashboard gaze_privacy/plugin_api_service.py gaze_privacy/reveal.py tests/python/test_plugin_api.py tests/python/test_reveal.py
git commit -m "feat: expose safe Hermes Desktop privacy API"
```

---

### Task 12: Hermes Desktop workspace shell, overview, live debug, and status indicator

**Files:**
- Create: `desktop/plugin.js`
- Create: `tests/desktop/plugin.test.mjs`
- Create: `package.json`

**Interfaces:**
- Consumes: `ctx.rest`, `ctx.socket`, `host.state.focusedSessionProfile`, backend API from Task 11.
- Produces: `/gaze-privacy` workspace route, sidebar item, status-bar item, Overview and Live Debug tabs.

- [ ] **Step 1: Write static/runtime contract tests**

The Desktop plugin is uncompiled ESM. Create a dependency-free Node test using `node:test` + `node:assert/strict` + `fs/promises`. It must assert:
- no JSX syntax;
- every bare import specifier is one of `@hermes/plugin-sdk`, `react`, `react/jsx-runtime`;
- route and sidebar contributions use the same `/gaze-privacy` path;
- no direct `65113`, `Authorization`, API-token-file, or master-key reference exists;
- the source imports exactly the SDK tab primitives `Tabs`, `TabsList`, and `TabsTrigger`.

Create `package.json` as:

```json
{
  "private": true,
  "type": "module",
  "scripts": {
    "test:desktop": "node --test tests/desktop/*.test.mjs"
  }
}
```

- [ ] **Step 2: Implement plugin registration**

Use a single `desktop/plugin.js`:

```javascript
import {
  ROUTES_AREA,
  SIDEBAR_NAV_AREA,
  Button,
  Codicon,
  StatusDot,
  Tabs,
  TabsList,
  TabsTrigger,
  useQuery,
  useQueryClient,
  useValue,
  host
} from '@hermes/plugin-sdk'
import { useEffect, useState } from 'react'
import { jsx, jsxs } from 'react/jsx-runtime'

const ID = 'gaze-hermes-privacy'
const PATH = '/gaze-privacy'

export default {
  id: ID,
  name: 'Gaze Privacy',
  register(ctx) {
    ctx.registerMany([
      {
        id: 'workspace',
        area: ROUTES_AREA,
        data: { path: PATH },
        render: () => jsx(PrivacyWorkspace, { ctx })
      },
      {
        id: 'nav',
        area: SIDEBAR_NAV_AREA,
        data: { path: PATH, label: 'Gaze Privacy', codicon: 'shield' }
      },
      {
        id: 'status',
        area: 'statusBar.right',
        render: () => jsx(PrivacyStatus, { ctx })
      }
    ])
  }
}
```

The current Hermes Desktop SDK exports `Tabs`, `TabsList`, and `TabsTrigger`; it does not export `TabsContent`. Keep the active tab in component-local `useState`, render triggers inside `TabsList`, and conditionally render the corresponding panel body below the list. Do not import private Radix primitives or app-internal modules.

- [ ] **Step 3: Implement Overview**

Fetch `/status` with React Query and show protection state, sidecar/NER versions, policy hash, counters, and Hermes capability state.

```javascript
function usePrivacyStatus(ctx) {
  return useQuery({
    queryKey: [ID, 'status'],
    queryFn: () => ctx.rest('/status'),
    refetchInterval: 3000
  })
}

function Overview({ ctx }) {
  const query = usePrivacyStatus(ctx)
  if (query.isPending) return jsx('div', { children: 'Loading privacy status' })
  if (query.isError) return jsx('div', { children: 'Privacy status unavailable' })
  const s = query.data
  return jsxs('div', {
    children: [
      jsx('h2', { children: s.protection_state }),
      jsx('div', { children: 'Sidecar: ' + s.sidecar.mode + ' ' + s.sidecar.version }),
      jsx('div', { children: 'Policy: ' + s.policy_hash }),
      jsx('div', { children: 'Protected ' + s.counters.protected + ' · Bypassed ' + s.counters.bypassed + ' · Blocked ' + s.counters.blocked }),
      jsx('div', { children: 'Fail-closed ' + (s.capabilities.fail_closed ? 'yes' : 'no') + ' · Stream transform ' + (s.capabilities.stream_text ? 'yes' : 'no') })
    ]
  })
}
```

No hard-coded colours; use SDK components and theme variables for any styling added around this structure.

- [ ] **Step 4: Implement Live Debug**

Use `ctx.socket('/events', onEvent)` when available and React Query polling fallback because Desktop sockets are no-op on OAuth remotes. `onEvent` only invalidates the sanitised query.

```javascript
function usePrivacyEvents(ctx) {
  const queryClient = useQueryClient()
  const query = useQuery({
    queryKey: [ID, 'events'],
    queryFn: () => ctx.rest('/events?limit=200'),
    refetchInterval: 5000
  })

  useEffect(() => {
    return ctx.socket('/events', () => {
      void queryClient.invalidateQueries({ queryKey: [ID, 'events'] })
    })
  }, [ctx, queryClient])

  return query
}
```

The event renderer displays request ID, provider/model, PII classes/counts, decision, latency, streaming state and errors only. It never writes event bodies to `ctx.storage`.

- [ ] **Step 5: Implement status-bar state**

Map current status to `Protected`, `Local bypass`, `Blocked`, or `Error`. Clicking the status item navigates to `/gaze-privacy`.

```javascript
function PrivacyStatus({ ctx }) {
  const query = usePrivacyStatus(ctx)
  const label = query.isError
    ? 'Error'
    : query.isPending
      ? 'Checking privacy'
      : query.data.protection_state
  return jsx(Button, {
    variant: 'ghost',
    onClick: () => host.navigate(PATH),
    children: label
  })
}
```

- [ ] **Step 6: Run Desktop tests**

Run:

```bash
node --check desktop/plugin.js
npm run test:desktop
```

Expected: both commands exit 0; the static contract suite confirms the plugin uses the public SDK surface, shares one route path, and never references the sidecar endpoint or secrets directly.

- [ ] **Step 7: Commit**

```bash
git add desktop tests/desktop package.json
git commit -m "feat: add Hermes Desktop privacy workspace"
```

---

### Task 13: Desktop Rules, Test Lab, Providers, Sessions, and temporary sensitive reveal UI

**Files:**
- Modify: `desktop/plugin.js`
- Modify: `tests/desktop/plugin.test.mjs`
- Create: `tests/desktop/reveal.test.mjs`
- Create: `tests/desktop/policy-editor.test.mjs`

**Interfaces:**
- Consumes: Task 11 policy/provider/session/reveal endpoints.
- Produces: visual + raw TOML editor, semantic diff/apply flow, local Test Lab, provider trust editor, session management, ephemeral reveal state.

- [ ] **Step 1: Write policy-editor tests**

The Node tests remain dependency-free source-contract tests. Add:

```javascript
test('policy editor uses edit validate apply flow', async () => {
  const source = await readFile(new URL('../../desktop/plugin.js', import.meta.url), 'utf8')
  assert.match(source, /\/policies\/validate/)
  assert.match(source, /\/policies\/edit/)
  assert.match(source, /\/policies\/apply/)
  assert.match(source, /Advanced TOML/)
  assert.match(source, /focusedSessionProfile/)
})
```

Backend Task 11 tests verify semantic policy preservation; the Desktop test pins that the UI uses the safe edit/validate/apply path rather than writing files directly.

- [ ] **Step 2: Implement Rules tab**

Use two modes, `Visual` and `Advanced TOML`. The visual editor exposes only fields that map losslessly to Gaze 0.14 policy TOML: recogniser name/kind/pattern/class, dictionary source, case sensitivity/token family, rule identity/action, locales, and NER settings. Do not invent recogniser `enabled`, `priority`, `scope`, or `description` keys.

```javascript
async function applyVisualEdit(ctx, scope, edit) {
  const edited = await ctx.rest('/policies/edit', {
    method: 'POST',
    body: { scope, edit }
  })
  const validated = await ctx.rest('/policies/validate', {
    method: 'POST',
    body: { scope, toml: edited.toml }
  })
  if (!validated.valid) throw new Error('Policy validation failed')
  return ctx.rest('/policies/apply', {
    method: 'POST',
    body: { scope, toml: edited.toml, expected_hash: edited.base_hash }
  })
}
```

Profile inheritance/removal is represented by the plugin overlay schema from Task 4. Any valid construct not represented by the visual editor remains Advanced-only and byte-preserved. Both modes show the semantic diff before the final Apply action.

- [ ] **Step 3: Implement Test Lab**

POST the unsaved draft plus sample text to `/policies/test` and render the local round trip:

```javascript
async function runPolicyTest(ctx, scope, toml, sample) {
  return ctx.rest('/policies/test', {
    method: 'POST',
    body: { scope, toml, sample }
  })
}
```

Display original sample, detections, protected text, restored text, round-trip status, and responsible rule. The backend endpoint is sidecar-local and never calls an LLM provider.

- [ ] **Step 4: Implement Providers and Sessions tabs**

Providers display `PROTECTED`, `TRUSTED LOCAL`, or `BLOCKED / UNSUPPORTED`. After explicit confirmation, update trust with:

```javascript
await ctx.rest('/providers/' + encodeURIComponent(providerId) + '/trust', {
  method: 'PUT',
  body: { trusted_local: nextTrusted }
})
```

Sessions display profile/session ID, timestamps, snapshot state, mapping count, policy version, and recover/reset/delete actions. After confirmation, deletion uses:

```javascript
await ctx.rest(
  '/sessions/' + encodeURIComponent(profileId) + '/' + encodeURIComponent(sessionId),
  { method: 'DELETE' }
)
```

Reset/delete confirmation text states that previous reversible mappings will be abandoned.

- [ ] **Step 5: Add Review Focus reveal lifecycle tests**

Pin the UI reveal lifecycle in the source-contract test:

```javascript
test('reveal state is ephemeral and auto-cleared', async () => {
  const source = await readFile(new URL('../../desktop/plugin.js', import.meta.url), 'utf8')
  assert.match(source, /REVEAL_TTL_MS\s*=\s*60_000/)
  assert.match(source, /clearTimeout/)
  assert.match(source, /focusedSessionProfile/)
  assert.doesNotMatch(source, /ctx\.storage\.set\([^)]*reveal/i)
})
```

Backend Task 11 tests enforce grant expiry and profile binding. Manual Desktop QA in Task 15 additionally verifies clearing on workspace unmount and backend/gateway disconnect.

- [ ] **Step 6: Implement reveal control**

The button requests `POST /events/{id}/reveal`, keeps returned sensitive data only in component-local state, and schedules clearing:

```javascript
const REVEAL_TTL_MS = 60_000

function useSensitiveReveal(profileId) {
  const [revealed, setRevealed] = useState(null)

  useEffect(() => {
    setRevealed(null)
  }, [profileId])

  useEffect(() => {
    if (!revealed) return undefined
    const timer = setTimeout(() => setRevealed(null), REVEAL_TTL_MS)
    return () => clearTimeout(timer)
  }, [revealed])

  return { revealed, setRevealed }
}
```

The workspace also clears `revealed` when the backend/gateway connection state becomes disconnected. No reveal body enters query-cache persistence, URL state, logs, or `ctx.storage`.

- [ ] **Step 7: Run Desktop tests**

Run:

```bash
node --check desktop/plugin.js
npm run test:desktop
```

Expected: all editor, provider, session, and reveal contract tests pass.

- [ ] **Step 8: Commit**

```bash
git add desktop/plugin.js tests/desktop
git commit -m "feat: add privacy policy and session controls"
```

---

### Task 14: Docker packaging, release artifacts, CI, and supply-chain verification

**Files:**
- Create: `docker/Dockerfile`
- Create: `docker-compose.yml`
- Create: `.github/workflows/test.yml`
- Create: `.github/workflows/release.yml`
- Create: `scripts/update-sidecar-manifest.py`
- Create: `scripts/docker-smoke.sh`
- Modify: `sidecar-release.json`
- Create: `tests/python/test_release_manifest.py`

**Interfaces:**
- Consumes: sidecar binary and Python plugin.
- Produces: native binaries, Docker image, checksums, release manifest, CI gates.

- [ ] **Step 1: Write release-manifest tests**

Reject unsupported platform tuples, duplicate artifact tuples, malformed SHA-256 values, protocol mismatches, and non-HTTPS release URLs outside local fixtures.

```python
def test_release_manifest_rejects_bad_digest(tmp_path):
    manifest = {
        "protocol_version": 1,
        "artifacts": [{
            "os": "linux",
            "arch": "x86_64",
            "url": "https://example.invalid/sidecar",
            "sha256": "not-a-digest",
        }],
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps(manifest), encoding="utf-8")
    with pytest.raises(ManifestError, match="sha256"):
        load_release_manifest(path)


def test_release_manifest_rejects_duplicate_target(tmp_path):
    artifact = {
        "os": "linux",
        "arch": "x86_64",
        "url": "https://example.invalid/sidecar",
        "sha256": "0" * 64,
    }
    path = tmp_path / "manifest.json"
    path.write_text(json.dumps({"protocol_version": 1, "artifacts": [artifact, artifact]}), encoding="utf-8")
    with pytest.raises(ManifestError, match="duplicate"):
        load_release_manifest(path)
```

- [ ] **Step 2: Implement Docker image**

Build the same Rust sidecar binary used by native installs. Use:

```dockerfile
FROM rust:1.89-bookworm AS build
WORKDIR /src
COPY sidecar ./sidecar
RUN cargo build --locked --release --manifest-path sidecar/Cargo.toml

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --create-home gaze
COPY --from=build /src/sidecar/target/release/gaze-hermes-sidecar /usr/local/bin/gaze-hermes-sidecar
USER 10001:10001
EXPOSE 65113
ENTRYPOINT ["/usr/local/bin/gaze-hermes-sidecar"]
```

Compose runs the container with `--bind 0.0.0.0:65113`, mounts API-token/key/policy files read-only, mounts the encrypted state directory read-write, and publishes only:

```yaml
ports:
  - "127.0.0.1:65113:65113"
```

The container may listen on all container interfaces because the host publish is loopback-only; native mode remains loopback-bound directly.

- [ ] **Step 3: Implement CI test matrix**

Use an explicit GitHub Actions matrix:

```yaml
strategy:
  fail-fast: false
  matrix:
    include:
      - os: ubuntu-latest
        target: x86_64-unknown-linux-gnu
      - os: ubuntu-24.04-arm
        target: aarch64-unknown-linux-gnu
      - os: macos-15-intel
        target: x86_64-apple-darwin
      - os: macos-15
        target: aarch64-apple-darwin
      - os: windows-latest
        target: x86_64-pc-windows-msvc
```

Each applicable job runs Python tests, `cargo test --target`, and a release build. A Linux quality job additionally runs `cargo fmt --check`, clippy, Desktop ESM tests, synthetic e2e tests, Docker smoke, and a grep-based forbidden-fixture scan over captured logs. If a named hosted runner is unavailable in the target GitHub organisation, replace it with an equivalent GitHub-hosted runner for the same architecture, without dropping the target.

- [ ] **Step 4: Implement release workflow**

Build the five native targets above, produce SHA-256 values, attach binaries, publish the Docker image, and generate `sidecar-release.json` only from successful artifacts.

```yaml
- name: Hash artifact
  shell: bash
  run: sha256sum "$ARTIFACT" > "$ARTIFACT.sha256"

- name: Update sidecar manifest
  run: >
    python scripts/update-sidecar-manifest.py
    --protocol-version 1
    --artifacts-dir dist
    --output sidecar-release.json
```

The manifest updater refuses a target without both a binary and checksum. Release publication depends on the full test workflow and never fabricates a missing platform entry.

- [ ] **Step 5: Add and run the Docker health/auth smoke test**

`scripts/docker-smoke.sh` must create a temporary synthetic API token, 32-byte snapshot key, and a minimal no-NER Gaze test policy, start the Compose service, then assert:

```bash
curl --fail http://127.0.0.1:65113/healthz
curl --fail-with-body http://127.0.0.1:65113/v1/status && exit 1 || test "$?" -eq 22
curl --fail -H "Authorization: Bearer $GAZE_TEST_TOKEN" http://127.0.0.1:65113/v1/status
```

The script must trap cleanup and run `docker compose down -v` on exit. The unauthenticated `/v1/status` request must return HTTP 401; `/healthz` stays minimal and unauthenticated.

Run:

```bash
pytest tests/python/test_release_manifest.py -v
cargo test --manifest-path sidecar/Cargo.toml
docker compose config
bash scripts/docker-smoke.sh
```

Expected: all commands exit 0.

- [ ] **Step 6: Commit**

```bash
git add docker docker-compose.yml .github scripts sidecar-release.json tests/python/test_release_manifest.py
git commit -m "build: add verified sidecar release pipeline"
```

---

### Task 15: End-to-end privacy regression suite, documentation, and release gate

**Files:**
- Create: `tests/fixtures/pii-regression.json`
- Create: `tests/e2e/test_privacy_boundary.py`
- Create: `tests/e2e/test_tool_restore.py`
- Create: `tests/e2e/test_remote_profile_isolation.py`
- Create: `tests/e2e/test_logging.py`
- Modify: `README.md`
- Create: `SECURITY.md`
- Create: `docs/architecture.md`
- Create: `docs/install.md`
- Create: `docs/debugging.md`
- Create: `docs/threat-model.md`

**Interfaces:**
- Consumes: complete plugin, patched/supported Hermes, sidecar, Desktop/API contracts.
- Produces: release-grade evidence that the trust boundary works end to end.

- [ ] **Step 1: Build a synthetic-only regression corpus**

Use a versioned JSON fixture file with only synthetic data:

```json
{
  "schema_version": 1,
  "cases": [
    {
      "id": "email-markdown",
      "text": "Contact Ada Example at ada@example.invalid in York.",
      "must_protect": ["Ada Example", "ada@example.invalid", "York"]
    },
    {
      "id": "custom-order-json",
      "text": "{\"customer\":\"Synthetic Person\",\"order\":\"ORD-123456\"}",
      "must_protect": ["Synthetic Person", "ORD-123456"]
    },
    {
      "id": "latex-tool",
      "text": "\\author{Synthetic Author}\\email{author@example.invalid}",
      "must_protect": ["Synthetic Author", "author@example.invalid"]
    }
  ]
}
```

Expand the corpus with synthetic phone, postal/location, organisation, multilingual/rulepack-supported, XML, source-code, nested-payload, tool-call, and awkward stream-split cases. No record may contain real personal data.

- [ ] **Step 2: Write an external-provider capture test**

Run Hermes against a local capture server whose provider ID is deliberately **not** trusted:

```python
def test_external_provider_capture_contains_no_raw_pii(hermes_privacy_harness, pii_cases):
    capture = hermes_privacy_harness.fake_external_provider()
    for case in pii_cases:
        result = hermes_privacy_harness.run(case["text"], provider=capture.provider_id)
        sent = capture.last_request_bytes()
        for raw in case["must_protect"]:
            assert raw.encode() not in sent
        assert b"<" in sent or b"gaze-fake.invalid" in sent
        assert all(raw in result.visible_text for raw in case["must_protect"])
        assert not hermes_privacy_harness.logs_contain(case["must_protect"])
```

Also assert persisted events contain only request IDs, provider/model, classes/counts, policy hash, decision, latency and error codes.

- [ ] **Step 3: Write the LaTeX/tool-call acceptance test**

Fake provider returns a streamed `write_file` call whose argument token is split across provider chunks:

```python
def test_streamed_write_file_arguments_restore_before_execution(hermes_privacy_harness, tmp_path):
    provider = hermes_privacy_harness.fake_tool_provider(
        tool_name="write_file",
        output_path=tmp_path / "letter.tex",
        split_every_byte=True,
    )
    result = hermes_privacy_harness.run(
        "Write a LaTeX letter for Synthetic Author, author@example.invalid",
        provider=provider.provider_id,
    )
    tool_call = result.executed_tools[-1]
    args = json.loads(tool_call.arguments)
    assert args["content"].find("Synthetic Author") >= 0
    assert args["content"].find("author@example.invalid") >= 0
    assert (tmp_path / "letter.tex").read_text("utf-8") == args["content"]
```

- [ ] **Step 4: Write crash/recovery acceptance test**

Exercise both successful and refused recovery:

```python
def test_sidecar_restart_recovers_session_but_wrong_key_blocks(harness):
    token = harness.clean_and_persist("Synthetic Person <synthetic@example.invalid>")
    harness.kill_sidecar()
    harness.restart_sidecar(same_key=True)
    assert harness.restore(token) == "Synthetic Person <synthetic@example.invalid>"

    harness.kill_sidecar()
    harness.restart_sidecar(same_key=False)
    with pytest.raises(PrivacyBlockedError):
        harness.send_external_followup(token)
```

- [ ] **Step 5: Write multi-profile isolation acceptance test**

Profile A's token must not restore under Profile B, and dedicated mode must allocate separate endpoints:

```python
def test_profile_namespaces_and_dedicated_sidecars_are_isolated(harness):
    a = harness.profile("alpha", sidecar_scope="profile")
    b = harness.profile("beta", sidecar_scope="profile")
    token = a.clean("alpha@example.invalid")
    assert a.sidecar_endpoint != b.sidecar_endpoint
    with pytest.raises(StrictRestoreError):
        b.restore(token)
    assert set(a.snapshot_files()).isdisjoint(set(b.snapshot_files()))
```

- [ ] **Step 6: Write mandatory compatibility acceptance test**

Test both host versions explicitly:

```python
def test_unpatched_hermes_blocks_external_before_provider(unpatched_harness):
    provider = unpatched_harness.fake_external_provider()
    with pytest.raises(PrivacyBlockedError):
        unpatched_harness.run("alice@example.invalid", provider=provider.provider_id)
    assert provider.request_count == 0


def test_patched_hermes_reports_required_capabilities(patched_harness):
    caps = patched_harness.privacy_status()["capabilities"]
    assert caps["fail_closed"] is True
    assert caps["stream_text"] is True
```

- [ ] **Step 7: Write logging leak test**

Capture Python and Rust logs while running the corpus:

```python
def test_logs_never_contain_sensitive_material(harness, pii_cases):
    harness.run_corpus(pii_cases)
    logs = harness.all_logs()
    forbidden = [
        raw
        for case in pii_cases
        for raw in case["must_protect"]
    ] + [harness.api_token, harness.master_key_text]
    for value in forbidden:
        assert value not in logs
```

- [ ] **Step 8: Write operator documentation**

Write the docs with these required headings:

```text
README.md
  Install in Hermes
  How protection works
  Trusted local providers
  Desktop privacy console
  Compatibility and Hermes version

docs/install.md
  Native-first installation
  Docker mode
  External sidecar mode
  NER first-run download
  Offline model provisioning
  Secret files and generated fallbacks

docs/architecture.md
  Trust boundary
  Request clean flow
  Streaming restore flow
  Session persistence
  Profile isolation
  Policy layering

docs/debugging.md
  Health and protocol checks
  Blocked external request diagnostics
  Policy validation
  Snapshot recovery
  Sensitive reveal controls

docs/threat-model.md
  Trusted components
  Untrusted external providers
  Protected data
  Out-of-scope host compromise
  Failure-closed guarantees
```

`SECURITY.md` documents vulnerability reporting and never claims that installing the plugin by itself makes a deployment GDPR compliant. The threat model explicitly states that Hermes/local host are trusted and external model providers are outside the privacy boundary.

Manual Desktop QA before release: reveal one synthetic event, switch profile, verify it clears; reveal again, navigate away/unmount, verify it clears; reveal again, disconnect the backend, verify it clears.

- [ ] **Step 9: Run the full release gate**

```bash
pytest tests/python tests/e2e -v
cargo fmt --manifest-path sidecar/Cargo.toml -- --check
cargo clippy --manifest-path sidecar/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path sidecar/Cargo.toml
```

Then run:

```bash
node --check desktop/plugin.js
npm run test:desktop
docker compose config
bash scripts/docker-smoke.sh
```

Expected: every suite passes, no PII/log leak assertion fires, and the fake external provider never receives original protected values.

- [ ] **Step 10: Commit**

```bash
git add tests README.md SECURITY.md docs
git commit -m "test: certify end-to-end privacy boundary"
```

---

## Final integration sequence

After all task commits:

1. Rebase the plugin worktree on the latest `gaze-hermes-privacy` main branch.
2. Rebase the Hermes upstream branch on the current `NousResearch/hermes-agent` main branch and rerun Task 1's focused suites.
3. Run the plugin full release gate against that exact Hermes revision.
4. Record the tested Hermes commit and upstream PR URL in `docs/upstream-hermes.md`.
5. Open/update the upstream Hermes PR.
6. Open a `gaze-hermes-privacy` release PR containing only reviewed task commits.
7. Run CI from a clean checkout before merge.
8. Merge only when both the privacy boundary suite and ordinary Hermes middleware/stream regressions are green.

## Definition of Done

The implementation is complete only when all of the following are demonstrated in automated tests:

- external providers never receive original synthetic PII for supported payloads;
- unsupported/uninspectable external payloads block in mandatory mode;
- trusted-local bypass is explicit and exact-match only;
- provider fallback reevaluates trust;
- live text/reasoning/commentary is restored before Desktop/TTS delivery;
- completed responses are restored before Hermes tool parsing;
- streamed tool arguments produce valid restored tool calls;
- sidecar failure cannot trigger fail-open provider execution on supported Hermes;
- restart recovery uses encrypted snapshots and rejects wrong keys/tampering;
- policy edits cannot replace a working policy until validation succeeds;
- Desktop sensitive reveal is ephemeral and non-persistent;
- default install works without Docker, Rust, or a separate Gaze installation;
- Docker remains a supported optional deployment;
- ordinary logs contain no raw sensitive values or secrets.
