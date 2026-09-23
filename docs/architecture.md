# Architecture

## Trust Boundary

```
┌─────────────────────────────────────────────────────────────┐
│                    TRUSTED ZONE                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Hermes    │───▶│   Middle-   │───▶│  Gaze Sidecar   │  │
│  │   Host      │    │   ware      │    │  (Local)        │  │
│  └─────────────┘    └─────────────┘    └────────┬────────┘  │
│                                                  │            │
└──────────────────────────────────────────────────┼────────────┘
                                                   │
                    UNTRUSTED ZONE                 ▼
            ┌─────────────────────────────────────────────────────┐
            │           External LLM Providers                    │
            │  (OpenRouter, Anthropic, OpenAI, etc.)              │
            └─────────────────────────────────────────────────────┘
```

The trust boundary is between the local Gaze sidecar and external LLM providers. All components inside the trusted zone (Hermes host, middleware, sidecar) are considered trustworthy. External providers are explicitly untrusted.

## Request Clean Flow

1. **Extraction**: Hermes middleware extracts text fields from the request payload using API-mode-specific adapters (chat_completions, anthropic_messages, codex_responses, bedrock_converse).

2. **Cleaning**: Extracted fields are sent to the Gaze sidecar via `POST /v1/clean`. The sidecar tokenizes PII using Gaze policies and returns cleaned text with tokens.

3. **Application**: The middleware applies cleaned text back to the request payload using JSON-pointer paths, preserving all structural fields (model IDs, roles, tool names, schema definitions).

4. **Provider Call**: The cleaned request is sent to the external provider.

5. **Restoration**: The provider's response is restored via `POST /v1/restore` before Hermes parses tool calls or delivers to UI.

## Streaming Restore Flow

For streaming responses (`llm_stream_text` middleware):

1. **Reserve**: Before the provider call, a stream registry entry is created keyed by `(profile_id, session_id, api_request_id)`.

2. **Feed**: Each streaming chunk is fed to the sidecar's WebSocket `/v1/streams/{id}` via `StreamClient.feed(kind, text)`.

3. **Restore**: The sidecar restores tokens in real-time using per-lane carry buffers that handle split tokens across chunks.

4. **Finish**: On stream completion, remaining lane buffers are strictly restored.

5. **Abort**: On error, stream state is cleaned up.

## Session Persistence

Sessions are encrypted and persisted to disk:

- **Encryption**: ChaCha20-Poly1305 with per-session keys derived from master key
- **Namespace**: `profile_id` + `session_id` (request_id excluded)
- **Atomic writes**: Write to temp file, `sync_all()`, then atomic rename
- **Recovery**: On startup, sessions are decrypted and restored. Corrupt snapshots block recovery until explicit delete/reset.

## Profile Isolation

Profiles provide namespace isolation:

- **Host scope**: Single sidecar shared across profiles (default)
- **Profile scope**: Dedicated sidecar process per profile with ephemeral port and ready-file discovery
- **Snapshots**: Separate encrypted snapshot directories per profile
- **Sidecar endpoints**: Profile scope uses `127.0.0.1:0` with ready-file for port discovery

## Policy Layering

Policies support global + per-profile overrides:

- **Global policy**: Base rules for all profiles
- **Profile overlay**: Recognizers keyed by name, class rules by `class:<class>`, column rules by `column:<column>`
- **Removal**: `remove_recognizers` and `remove_rules` delete inherited entries
- **Preservation**: Unknown/advanced TOML keys are byte-preserved through edit/validate/apply flow
- **Validation**: All policies validated against Gaze 0.14 schema before activation
- **Atomic apply**: CAS via SHA-256 hash ensures no stale overwrites