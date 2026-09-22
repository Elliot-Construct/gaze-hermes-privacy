# Hermes Gaze Privacy Design

**Date:** 2026-09-22  
**Repository:** `Elliot-Construct/gaze-hermes-privacy`  
**Status:** Conversational design approved; written specification awaiting final review

## 1. Purpose

`gaze-hermes-privacy` is an open-source privacy layer for Hermes Agent that prevents personally identifiable information (PII) from being transmitted to external LLM providers while preserving normal Hermes behaviour locally.

Hermes itself remains inside the trusted boundary. Local Hermes processes, tools, files, memory, MCP servers, terminal commands, and document-generation workflows may continue to use real PII. The plugin applies reversible pseudonymisation specifically at the boundary where Hermes sends data to an external model provider, then restores the provider response locally before Hermes processes assistant content or tool calls.

The project must remain vendor-neutral. It must not contain company-specific branding, policy defaults, organisation names, deployment assumptions, or proprietary configuration.

## 2. Success Criteria

The design succeeds when:

1. External model providers receive pseudonymised content instead of the original PII.
2. Hermes receives restored content before assistant output or tool calls are parsed.
3. Tool workflows such as `write_file`, terminal usage, MCP calls, and LaTeX generation operate on the original values without tool-specific privacy wrappers.
4. Provider routing, models, fallbacks, and credentials remain owned by Hermes.
5. Explicitly trusted local providers may bypass Gaze by policy.
6. Privacy failures block external transmission by default.
7. Streaming responses remain streaming from v1.
8. The default installation requires neither Docker, Rust, nor a separate Gaze installation.
9. Hermes Desktop provides native privacy status, debugging, policy editing, testing, provider controls, and session management.
10. Sensitive mappings and snapshots remain local and encrypted at rest.
11. The project remains independently installable as a standalone Hermes plugin repository.

## 3. Non-Goals

Version 1 will not:

- become an LLM provider or provider router;
- proxy all Hermes tools;
- redact PII from trusted local Hermes processing;
- infer that a provider is trusted merely because it uses localhost, Ollama, vLLM, or another local-looking endpoint;
- ship organisation-specific recognisers or policies;
- provide cloud fleet management;
- sync policy across unrelated Hermes hosts;
- send analytics or debugging data to an external telemetry service;
- require a permanent fork of Hermes;
- require Docker for normal installation.

## 4. Trust Boundary

The trusted zone includes Hermes, its local tools, its local files, and the local Gaze privacy runtime.

The untrusted boundary begins when data is about to leave the Hermes host for a model provider that is not explicitly marked trusted-local.

### Outbound path

```text
Hermes real context
      |
      v
Gaze detection
      |
      v
reversible pseudonymisation
      |
      v
external LLM provider
```

### Inbound path

```text
external provider stream
      |
      v
Gaze streaming restoration
      |
      v
Hermes restored assistant response
      |
      v
assistant parsing / tool parsing / execution
```

Restoration must happen before Hermes interprets tool calls.

For example, if a provider emits:

```text
write_file("letter.tex", "... Dear <PERSON_42> ...")
```

the plugin restores the response before Hermes parses it, so Hermes receives:

```text
write_file("letter.tex", "... Dear John Smith ...")
```

No privacy-specific modification is required inside `write_file` or other tools.

## 5. Selected Architecture

The selected architecture is a hybrid Hermes plugin plus a managed Rust sidecar.

```text
Hermes Desktop
     |
     | Hermes plugin API
     v
Hermes backend plugin (Python)
     |
     | authenticated loopback API
     v
Gaze privacy sidecar (Rust + Gaze crates)
```

The system contains three major components:

1. **Hermes backend plugin**
   - integrates with Hermes middleware;
   - resolves provider trust and effective policy;
   - manages sidecar lifecycle;
   - exposes safe plugin API routes to Hermes Desktop;
   - blocks protected calls when privacy cannot be established.

2. **Rust Gaze sidecar**
   - embeds Gaze as Rust crate dependencies;
   - performs PII detection, pseudonymisation, and restoration;
   - owns reversible mappings;
   - handles streaming restoration;
   - manages encrypted snapshots;
   - validates and activates policies;
   - emits sanitised local telemetry.

3. **Hermes Desktop plugin**
   - provides the operator UI;
   - displays health and debug information;
   - edits policies;
   - tests rules;
   - manages trusted providers and sessions;
   - never receives sidecar bearer credentials or encryption keys.

The sidecar is not a model proxy and does not own provider routing.

## 6. Repository Structure

The intended monorepo structure is:

```text
gaze-hermes-privacy/
├── plugin/
│   ├── agent/
│   │   ├── middleware.py
│   │   ├── sidecar_manager.py
│   │   ├── policy_resolver.py
│   │   ├── provider_policy.py
│   │   └── plugin_api.py
│   └── desktop/
│       └── plugin.js
├── sidecar/
│   ├── src/
│   │   ├── main.rs
│   │   ├── api/
│   │   ├── sessions/
│   │   ├── policies/
│   │   ├── streaming/
│   │   ├── crypto/
│   │   └── telemetry/
│   └── Cargo.toml
├── policies/
│   ├── default.toml
│   ├── strict.toml
│   └── examples/
├── docker/
├── scripts/
├── tests/
├── docs/
└── docker-compose.yml
```

Exact file boundaries may evolve during implementation, but the responsibilities above must remain separated.

## 7. Sidecar Deployment

### 7.1 Default: managed native binary

The default runtime is a prebuilt native Rust sidecar managed by the Hermes backend plugin.

Users do not need:

- Docker;
- a Rust compiler or Cargo;
- a separate Gaze installation.

Gaze libraries are compiled into the sidecar as Rust dependencies.

The plugin should:

1. identify the host platform and architecture;
2. obtain or locate the matching sidecar release artifact;
3. verify its checksum/signature metadata before execution;
4. provision the configured NER model if needed;
5. generate missing local secrets;
6. start the sidecar;
7. verify health and protocol compatibility.

### 7.2 Optional Docker deployment

Docker remains a first-class alternative for container-oriented hosts.

The reference Docker deployment binds the sidecar only to loopback:

```text
127.0.0.1:65113:65113
```

### 7.3 External sidecar mode

Advanced users may run the sidecar themselves. The plugin must still require the same protocol, authentication, health, and compatibility checks.

### 7.4 Multi-profile deployment

Default behaviour is one sidecar per Hermes host.

Every request is namespaced at minimum by:

- `profile_id`;
- `session_id`;
- `request_id` or equivalent turn/request identity.

Users may opt into one sidecar per Hermes profile for stronger process-level isolation.

## 8. Sidecar Authentication and Secrets

The local sidecar API requires bearer authentication.

Preferred production configuration uses a secret file, including Docker secret or bind-mounted secret file patterns.

If no API bearer secret exists, the plugin/sidecar generates a cryptographically random one and persists it locally. The value must never be logged.

Encrypted snapshot state uses a separate master encryption key.

Preferred production configuration is a secret file. If no key is supplied, a cryptographically random key is generated on first startup and persisted separately from the snapshot data. The application should emit a warning that an automatically generated local key is in use, but must never log the key value.

Filesystem permissions should be restrictive wherever the host platform supports them.

## 9. NER Model Provisioning

The stronger optional Gaze NER path is provisioned automatically on first setup.

The plugin or sidecar must:

1. use a pinned model version;
2. download from an expected release/source location;
3. verify a pinned cryptographic checksum before use;
4. refuse the downloaded asset if verification fails.

Manual/offline provisioning must also be supported.

Bundling the NER asset into every plugin release is not required.

## 10. Provider Trust Policy

All providers are treated as protected external providers unless explicitly allowlisted as trusted-local.

There is no automatic trust inference from:

- hostname;
- loopback IP;
- provider name;
- Ollama;
- vLLM;
- API shape;
- local network placement.

A stable Hermes provider identifier should be stored wherever Hermes exposes one.

Removing a trusted-local entry takes effect before the next provider execution.

Provider trust is evaluated separately for every provider execution attempt, including fallbacks.

Example:

```text
trusted local primary
    |
    | unavailable
    v
Hermes fallback selects external provider
    |
    v
Gaze protection required
```

Trust must never be inherited from the previous provider attempt.

## 11. Hermes Middleware Integration

The plugin uses Hermes LLM request/execution middleware to place privacy immediately around external provider execution.

Hermes remains responsible for:

- provider credentials;
- model selection;
- provider adapters;
- fallbacks;
- retries;
- tool definitions;
- request shape generation.

The plugin must avoid becoming a second provider abstraction.

### 11.1 Fail-closed requirement

Current Hermes execution middleware is fail-open for a middleware exception raised before the callback invokes `next_call()`: Hermes reports the middleware error and continues down the execution chain.

That behaviour is unsafe for a mandatory privacy boundary.

The project therefore requires a small upstream Hermes capability allowing middleware to explicitly register a fail-closed failure policy, conceptually:

```python
ctx.register_middleware(
    "llm_execution",
    protect_llm_call,
    failure_mode="closed",
)
```

Equivalent API naming is acceptable if it better matches upstream Hermes conventions.

Requirements for the upstream change:

- existing middleware retains current behaviour by default;
- fail-closed mode is opt-in per middleware registration;
- if fail-closed middleware fails before a successful downstream handoff, provider execution is aborted;
- tests prove the provider callback is never reached on fail-closed middleware failure;
- tests prove legacy fail-open behaviour remains unchanged;
- the behaviour is documented upstream.

This upstream task is tracked in repository Issue #1: **Upstream Hermes: add fail-closed LLM execution middleware**.

No permanent Hermes fork is permitted.

### 11.2 Older Hermes versions

Compatibility behaviour is configurable.

Default behaviour when the required fail-closed capability is absent:

- trusted-local providers may continue according to policy;
- external providers are blocked.

Users may explicitly opt into a weaker compatibility mode, but the plugin must clearly indicate that mandatory fail-closed protection is not guaranteed.

The plugin must never silently claim mandatory protection on an unsupported Hermes version.

## 12. Request Lifecycle

For a protected provider, the plugin must inspect the complete outbound provider payload, not merely the latest user message. Text-bearing fields that may contain PII include system prompts, conversation messages, tool-call arguments/results, tool descriptions, response-schema text, and other provider-specific textual fields.

The Python plugin is responsible for adapting Hermes/provider-native request and response objects into a canonical sidecar envelope and reconstructing the provider-native objects afterwards. The sidecar remains provider-agnostic and operates on explicitly identified transformable fields rather than acting as a provider/router.

For a protected provider:

1. Hermes prepares the provider request.
2. The plugin resolves:
   - active profile;
   - effective policy;
   - provider trust status;
   - sidecar health;
   - protocol compatibility.
3. The request is sent to the local sidecar for pseudonymisation.
4. The sidecar stores the reversible mapping internally.
5. Only the pseudonymised provider payload returns to Python.
6. Hermes invokes its configured provider normally.
7. The provider response stream is intercepted.
8. Provider chunks are fed into the sidecar streaming restoration channel.
9. Restored chunks are yielded to Hermes.
10. Hermes parses assistant content/tool calls from restored content.
11. Gaze session state is updated and persisted as needed.
12. Sanitised debugging/metrics events are recorded.

The reversible mapping must never be returned to the Python plugin in normal operation.

### 12.1 Structured and multimodal payloads

Structured payloads are supported only when every outbound field that may carry PII is either safely transformed or explicitly classified as non-sensitive metadata.

For v1, the privacy guarantee applies to supported text-bearing fields. Binary attachments, images, audio, opaque blobs, provider-native objects with unknown semantics, or any other field that Gaze cannot inspect must not be silently forwarded to an external provider in mandatory mode. The plugin must either:

- handle the field through a supported privacy transformation path; or
- block the request as an unsupported provider payload shape.

Trusted-local providers remain exempt according to the explicit allowlist.

The Desktop debug view should surface when a request was blocked because of unsupported multimodal or opaque content.

## 13. Sidecar REST API

All management endpoints require bearer authentication except `/healthz`, which may be unauthenticated only if it returns minimal non-sensitive health status.

Conceptual API:

```text
GET    /healthz

POST   /v1/clean
POST   /v1/restore

POST   /v1/policies/validate
POST   /v1/policies/test
POST   /v1/policies/reload
GET    /v1/policies/effective

GET    /v1/sessions
GET    /v1/sessions/{id}
DELETE /v1/sessions/{id}

GET    /v1/status
GET    /v1/metrics
```

Every privacy operation carries explicit namespace fields rather than relying on process-global state.

Example request:

```json
{
  "profile_id": "default",
  "session_id": "abc123",
  "request_id": "req456",
  "text": "Contact Jane at jane@example.com"
}
```

Example safe response:

```json
{
  "text": "Contact <PERSON_1> at <EMAIL_1>",
  "detections": [
    {"class": "person", "count": 1},
    {"class": "email", "count": 1}
  ]
}
```

The response must not include the reversible mapping.

## 14. Streaming Protocol

Streaming restoration is required in v1.

The plugin uses an authenticated WebSocket connection to the sidecar for each protected provider stream.

Provider-native stream objects are adapted by the Python plugin into canonical string-bearing deltas plus non-sensitive metadata. Only transformable textual deltas pass through the sidecar restoration state machine; the plugin then reconstructs the provider-native stream object before yielding it back to Hermes. Unknown or opaque stream fields are not assumed safe.

Conceptual endpoint:

```text
WS /v1/streams/{stream_id}
```

A stream is associated with:

- `profile_id`;
- `session_id`;
- `request_id`;
- provider;
- model.

The protocol has explicit lifecycle messages:

```text
open
chunk(seq=1)
chunk(seq=2)
...
finish
abort
```

The sidecar maintains carry-over state so protected placeholders split across provider chunks are never leaked or misparsed.

Example:

```text
chunk 1: "Hello <PER"
chunk 2: "SON_1>, your"
```

must be emitted to Hermes only after safe restoration as the equivalent original content.

Duplicate, missing, or out-of-order sequence numbers fail the stream rather than being guessed around.

## 15. Streaming Failure Handling

If restoration fails during an active stream, the plugin must:

1. stop yielding further content to Hermes;
2. abort/close the provider stream where the provider API allows it;
3. invalidate the active restoration stream;
4. preserve recoverable Gaze session state;
5. return a privacy restoration error;
6. emit a sanitised local debug event.

Content already emitted cannot be recalled, so the sidecar must retain enough boundary bytes to avoid releasing ambiguous partial protected tokens.

## 16. Fail-Closed Conditions

For a provider that is not explicitly trusted-local, the original request must not be transmitted when protection cannot be established.

The call is blocked on:

- sidecar unavailable;
- sidecar authentication failure;
- incompatible sidecar protocol;
- invalid effective policy;
- required NER/model unavailable;
- pseudonymisation failure;
- session recovery failure;
- snapshot decryption/integrity failure;
- namespace collision;
- middleware failure;
- unsupported provider request shape;
- streaming restoration failure;
- stream sequence corruption;
- malformed or unknown protected token;
- required fail-closed Hermes capability unavailable in mandatory mode.

The resulting Hermes-facing error must clearly identify the privacy layer and state whether any unprotected request was transmitted.

## 17. Policy Model

TOML is the canonical policy source.

Policy resolution is:

```text
bundled defaults
       |
       v
user global base policy
       |
       v
optional per-profile overrides
       |
       v
effective policy
```

Bundled defaults and user-owned policy are separate layers. Upgrades may replace bundled defaults but must never silently overwrite user policy files.

Per-profile policy must not require duplicating the entire base policy.

The sidecar supports built-in classes plus user-defined recognisers such as regex and dictionary rules, subject to Gaze capabilities.

## 18. Policy Activation

Policy changes use a safe activation lifecycle:

```text
edit
  |
  v
validate
  |
  v
preview semantic diff / test
  |
  v
atomic activate
```

An invalid policy must never replace the active policy.

Policy reload affects future detection decisions but does not destroy existing reversible mappings for active sessions.

Each activated policy has an immutable version/hash used by debugging and session metadata.

Recent local policy versions should be retained for rollback.

## 19. Session State and Persistence

Active reversible mappings live in memory for performance.

The sidecar also persists encrypted recovery snapshots.

Snapshot writes use an atomic file replacement pattern, conceptually:

```text
snapshot.tmp
    |
    | fsync
    v
atomic rename
    |
    v
snapshot.enc
```

Snapshots are namespaced by profile and session and contain an explicit format version.

The sidecar must never persist:

- plaintext original prompt;
- plaintext restored provider response;
- unencrypted PII mapping;
- master encryption key;
- sidecar bearer secret.

## 20. Session Recovery

After sidecar restart:

```text
request references session namespace
        |
        v
lookup in memory
        |
        | missing
        v
load encrypted snapshot
        |
        v
decrypt + integrity check + version check
        |
        v
restore Gaze session mapping
        |
        v
continue using same reversible mapping
```

If recovery is impossible, protected external calls for that session are blocked.

The sidecar must not silently create a fresh mapping for a session whose previous mapping is expected but unrecoverable.

Users may explicitly reset the privacy session from Hermes Desktop when they choose to abandon the previous mapping.

## 21. Hermes Desktop Integration

The repository ships as a hybrid Hermes plugin with both agent/backend and Desktop halves.

Hermes Desktop talks to the Python backend plugin through Hermes' plugin API namespace:

```text
/api/plugins/gaze-hermes-privacy/...
```

The Desktop renderer does not talk directly to `localhost:65113` and never receives the sidecar bearer credential.

Conceptual Hermes-facing endpoints include:

```text
/api/plugins/gaze-hermes-privacy/status
/api/plugins/gaze-hermes-privacy/events
/api/plugins/gaze-hermes-privacy/policies
/api/plugins/gaze-hermes-privacy/test
/api/plugins/gaze-hermes-privacy/providers
/api/plugins/gaze-hermes-privacy/sessions
```

The Python plugin deliberately exposes a narrower API than the sidecar.

## 22. Desktop Workspace

The main Hermes Desktop workspace is **Gaze Privacy** with:

```text
Overview
Live Debug
Rules
Test Lab
Providers
Sessions
```

### 22.1 Overview

Shows:

- protection state;
- sidecar health;
- sidecar deployment mode;
- sidecar/Gaze version;
- NER model version/status;
- effective policy version;
- protected/bypassed/blocked counts;
- fail-closed Hermes capability status.

### 22.2 Live Debug

Each LLM call is shown as a timeline:

```text
Hermes request
   |
   | PII detections
   v
Pseudonymised
   |
   v
Provider
   |
   v
Streaming restoration
   |
   v
Hermes
```

Default detail includes:

- timestamp;
- profile/session/request identity;
- provider/model;
- protected/bypassed/blocked state;
- detected PII classes and counts;
- input/output byte counts;
- clean latency;
- provider latency;
- restore latency;
- stream status;
- error category;
- policy version/hash.

Ordinary logs and debug events do not contain raw sensitive values.

### 22.3 Sensitive-data reveal

Sensitive values are hidden by default.

Desktop may provide an explicit **Reveal sensitive data** action.

Reveal behaviour:

- temporary;
- local UI state only;
- not persisted;
- not written to ordinary logs;
- automatically expires after a short timeout;
- immediately clears when the relevant pane closes, profile changes, or Desktop disconnects.

Sensitive reveal is a privileged inspection path, not normal telemetry.

### 22.4 Rules

Rules have synchronized views:

- **Visual editor** for normal rule management;
- **Advanced TOML** for the complete canonical representation.

The visual editor covers common fields such as:

- name;
- enabled state;
- recogniser/detector type;
- class/pattern/dictionary;
- action;
- priority;
- scope;
- description.

If valid TOML contains a construct the visual editor cannot safely represent, the UI marks it **Advanced-only** and preserves it rather than rewriting or dropping it.

Global inheritance and profile overrides must be visually distinct.

### 22.5 Test Lab

The Test Lab processes local sample text through the sidecar without contacting an external model.

It can display:

- original sample;
- detections/classes;
- pseudonymised output;
- restored output;
- round-trip result;
- rule responsible for each decision.

Users can test:

- global policy;
- one profile's effective policy;
- an unsaved draft policy.

### 22.6 Providers

Every discovered Hermes provider is shown as:

```text
PROTECTED
TRUSTED LOCAL
BLOCKED / UNSUPPORTED
```

Marking a provider trusted-local requires an explicit user action.

### 22.7 Sessions

Session metadata includes:

- profile;
- session ID;
- creation time;
- last-use time;
- snapshot status;
- mapping count;
- policy version;
- active/recoverable/invalid state.

Available actions:

- inspect;
- reset;
- delete persisted state;
- retry recovery.

Reset must clearly warn that it abandons the existing reversible mapping.

### 22.8 Status bar

Hermes Desktop also gets a compact privacy status contribution such as:

```text
Gaze: Protected
Gaze: Local bypass
Gaze: Blocked
Gaze: Error
```

Selecting it opens the Gaze Privacy workspace for the active profile.

## 23. Sanitised Telemetry

The plugin maintains a bounded local event buffer for debugging.

Allowed metadata includes:

- timestamps;
- profile/session/request IDs;
- provider/model;
- protection state;
- PII classes/counts;
- request and response sizes;
- latency measurements;
- stream state;
- policy version/hash;
- error category.

Forbidden in ordinary logs/event storage:

- plaintext prompts;
- plaintext model responses containing PII;
- reversible mappings;
- encryption keys;
- bearer secrets.

No external analytics service is part of v1.

## 24. Version and Protocol Compatibility

The plugin performs explicit compatibility checks for:

- Hermes middleware capability;
- required fail-closed support;
- Hermes Desktop SDK compatibility;
- sidecar protocol version;
- sidecar/Gaze runtime version;
- NER model version;
- encrypted snapshot format.

The backend plugin and sidecar negotiate a protocol version.

An incompatible sidecar is rejected rather than used optimistically.

Snapshot migrations must be explicit, versioned, and tested.

## 25. Release Packaging

A release may contain:

```text
gaze-hermes-privacy release
├── Hermes backend plugin
├── Hermes Desktop plugin
├── native sidecar artifacts
│   ├── linux-x86_64
│   ├── linux-aarch64
│   ├── windows-x86_64
│   └── macOS architectures supported by CI
├── Docker image
├── bundled default policies
└── release manifest/checksums
```

Exact platform coverage is determined by CI support, but platform absence must be reported clearly rather than silently falling back to an unverified artifact.

## 26. Updates

Plugin updates may replace:

- plugin code;
- native sidecar binary;
- Docker image;
- bundled defaults.

They must not silently replace:

- user global policy;
- profile overrides;
- locally generated keys/secrets;
- encrypted session snapshots.

The plugin verifies downloaded sidecar artifacts and NER assets before use.

## 27. Security and Disclosure

The repository should include:

- `SECURITY.md`;
- a clear vulnerability-reporting path;
- documentation of the trust boundary;
- documentation of compatibility mode limitations;
- documentation of what is and is not persisted;
- documentation of trusted-local bypass risks.

Security-sensitive defaults favour blocking over silent downgrade.

## 28. Testing Strategy

The test suite is divided into:

- detection;
- pseudonymisation;
- restoration;
- streaming;
- Hermes middleware integration;
- fail-closed enforcement;
- provider fallback;
- policy inheritance;
- snapshot encryption/recovery;
- Desktop plugin API;
- installation/update;
- cross-platform behaviour.

Required end-to-end cases include:

1. names, emails, phones, addresses, organisations, and supported classes are pseudonymised;
2. custom regex and dictionary recognisers work;
3. token mappings remain stable through a session;
4. external provider payload contains pseudonyms rather than original PII;
5. restored content reaches Hermes before tool parsing;
6. restored JSON/tool calls remain syntactically valid;
7. LaTeX/file-writing flows receive original values before file execution;
8. placeholders split across streaming chunks restore correctly;
9. multiple placeholders in one chunk restore correctly;
10. malformed or unknown protected tokens fail safely;
11. sidecar outage blocks external transmission;
12. authentication failure blocks external transmission;
13. incompatible sidecar protocol blocks external transmission;
14. invalid policy cannot replace the active policy;
15. corrupted or undecryptable snapshot cannot silently start a new mapping;
16. trusted-local bypass works only when explicitly configured;
17. fallback from trusted-local to external provider invokes protection;
18. profile namespaces cannot read each other's mappings;
19. dedicated per-profile sidecar mode works;
20. sensitive values do not appear in ordinary logs;
21. sensitive reveal state expires and is not persisted;
22. mandatory mode blocks external providers when Hermes lacks fail-closed support;
23. upstream Hermes fail-closed tests prove the provider callback is not invoked;
24. legacy Hermes fail-open middleware tests remain unchanged for non-critical middleware.

## 29. Privacy Regression Corpus

The repository includes a synthetic regression corpus covering:

- names;
- email addresses;
- phone numbers;
- postal addresses;
- organisations;
- custom identifiers;
- multilingual PII where supported;
- Markdown;
- JSON;
- XML;
- LaTeX;
- source code;
- assistant tool calls;
- nested structured payloads;
- intentionally awkward streaming boundaries.

The corpus must contain synthetic data only and must be safe to publish in the open-source repository.

## 30. Implementation Constraints

Implementation must preserve these architectural constraints:

- no Hermes fork;
- no provider routing inside the sidecar;
- no reversible mapping returned to the Python plugin during normal operation;
- no automatic trusted-provider inference;
- no plaintext snapshot storage;
- no sensitive values in ordinary logs;
- no external-provider transmission after a privacy failure in mandatory mode;
- no Desktop access to sidecar secrets;
- no organisation-specific defaults;
- no Docker requirement for the default install path.

## 31. Upstream Dependency

Repository Issue #1 tracks the upstream Hermes middleware change required for a strong fail-closed guarantee:

**Upstream Hermes: add fail-closed LLM execution middleware**

The implementation plan must treat this as a concrete workstream, including an upstream Hermes PR and integration tests in this repository.

The project may implement compatibility mode before the upstream change is merged, but mandatory external-provider protection must default to blocked until Hermes exposes the required fail-closed capability.

## 32. Acceptance Summary

Version 1 is acceptable when an end user can install the plugin, allow it to provision its verified native sidecar and NER asset, open Hermes Desktop, configure or test privacy policy, use an external provider with live streaming, and verify that the external provider receives only pseudonymised data while Hermes continues to operate on restored real values.

The same installation must work on a remote Hermes host without requiring Docker, while Docker remains available as an optional deployment mode.
