# Gaze Policy Configuration

## Overview

The sidecar loads a global policy document and optional per-profile overlay documents, merges them, and builds a Gaze pipeline from the effective result.

## Layout

```
policies/
  global.toml           # active global policy (runtime store root)
  profiles/
    <profile_id>.toml   # profile overlay
```

Shipped defaults live in `policies/default.toml` and `policies/strict.toml`.

## Global policy (Gaze 0.14)

Validated with `gaze::Policy::load`. Key sections:

- `schema_version = "0.1.0"` (required)
- `[session]` — `scope = "ephemeral" | "conversation" | "persistent"`
- `[policy.rulepacks]` — `bundled` list (`core`, `core-extended`) and optional `paths`
- `[[policy.custom_recognizers]]` — regex/dictionary recognizers
- `[[rule]]` — `kind = "class" | "column" | "default"`, `action = "tokenize" | "redact" | ...`
- `[ner]` — optional `model_dir` / `locale` / `threshold` (NER only active when `model_dir` set)

Unknown keys are rejected by Gaze's `deny_unknown_fields` policy structs.

## Profile overlay schema

`schema_version = "gaze-hermes-profile-1"`. Overlay is never loaded as a Gaze policy directly — only the merged output is.

```toml
schema_version = "gaze-hermes-profile-1"

remove_rules = ["class:email", "column:ssn", "default"]
remove_recognizers = ["legacy_rec"]

[[overrides.rules]]
kind = "class"
class = "email"
action = "redact"

[[overrides.recognizers]]
kind = "regex"
name = "tenant_order"
pattern = 'TXN-[0-9]+'
class = "custom:order_id"
```

### Identity rules

- Recognizers keyed by `name`
- Class rules keyed by `class:<class>`
- Column rules keyed by `column:<column>`
- Default rules keyed by `default`
- Profile replacements override matching global entries
- `remove_rules` / `remove_recognizers` delete inherited entries
- Unrelated/advanced global TOML and comments are preserved (`toml_edit`)

## Optimistic concurrency

- `PolicyStore::document_hash(scope)` returns sha256 hex of the scope's own document
- `edit` returns a candidate without mutating active state
- `apply` checks `expected_hash`, builds candidate pipeline first, then atomically writes
- Stale hash or invalid candidate leaves file and in-memory state unchanged

## NER provisioning

`ModelProvisioner::ensure()` returns a verified model directory. Production uses `gaze_model_setup::install_kiji_bundle` (default dir `kiji-distilbert`). Tests use fake provisioners and never hit the network.
