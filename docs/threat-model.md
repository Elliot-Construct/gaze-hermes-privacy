# Threat Model

## Trusted Components

The following components are considered trusted and within the security boundary:

| Component | Trust Level | Rationale |
|-----------|-------------|-----------|
| Hermes Host Application | Trusted | Runs on operator's machine, controlled by operator |
| Gaze Sidecar (Local) | Trusted | Runs on operator's machine, open-source, auditable |
| Gaze Library (v0.14.0) | Trusted | Pinned version, deterministic behavior |
| Local Trusted Providers | Trusted | Explicit operator consent, exact-match only |
| Operator Secrets | Trusted | Generated with CSPRNG, 0600 permissions |
| Encrypted Snapshots | Trusted | ChaCha20-Poly1305, per-session keys |

## Untrusted External Providers

The following are explicitly untrusted and outside the privacy boundary:

| Provider Type | Trust Level | Mitigation |
|---------------|-------------|------------|
| OpenRouter | Untrusted | Request cleaning, response restoration |
| Anthropic API | Untrusted | Request cleaning, response restoration |
| OpenAI API | Untrusted | Request cleaning, response restoration |
| Any external LLM API | Untrusted | Request cleaning, response restoration |

**No external provider ever receives original PII.** All text fields are tokenized before leaving the trust boundary.

## Protected Data

The following data types are protected by the privacy boundary:

| Data Type | Protection Method | Scope |
|-----------|-------------------|-------|
| Email addresses | Gaze regex + NER | Request/Response |
| Names (person) | Gaze NER + custom rules | Request/Response |
| Phone numbers | Gaze regex | Request/Response |
| Postal addresses | Gaze NER | Request/Response |
| Organizations | Gaze NER | Request/Response |
| Locations | Gaze NER | Request/Response |
| Custom identifiers | User-defined regex/dictionary | Request/Response |
| Tool call arguments | Streaming restoration | Streaming response |
| Completed responses | Full restoration | Response body |

**Not Protected** (intentionally):
- Model names/IDs
- Role identifiers (user/assistant/system/tool)
- Content part types (text, image_url, input_audio)
- Tool/function names
- JSON schema definitions
- Request IDs, timestamps, metadata

## Out of Scope (Host Compromise)

The following are explicitly OUT OF SCOPE for this threat model:

| Threat | Reason |
|--------|--------|
| Root access to Hermes host | Full system compromise defeats all software controls |
| Sidecar binary replacement | Requires host compromise |
| Memory scraping of Hermes process | Requires host compromise |
| Network interception (Hermes↔Sidecar) | Requires host/local network compromise |
| Operator coercion | Human factor, not technical |
| Supply chain attack on Gaze crate | Mitigated by pinned version + checksums |
| Vulnerabilities in Gaze library | Mitigated by version pinning + monitoring |

## Failure-Closed Guarantees

The system provides the following failure-closed guarantees:

| Failure Scenario | Behavior |
|------------------|----------|
| Sidecar process crashes | All external calls blocked until restart |
| Sidecar returns error | Request blocked, error returned to Hermes |
| Sidecar returns invalid tokens | Request blocked, validation fails |
| Network partition (Hermes↔Sidecar) | Calls blocked until connectivity restored |
| Hermes lacks `fail_closed` capability | Mandatory mode: block external; Compatibility: warn |
| Sidecar protocol mismatch | Calls blocked until versions align |
| Master key corruption | All snapshots unreadable, new sessions only |
| NER model missing | NER disabled, regex-only protection continues |

## Data Flow Summary

```
User Input ──▶ Hermes ──▶ Middleware ──▶ Extract Fields ──▶ Gaze Sidecar ──▶ Cleaned Request ──▶ External Provider
     ▲                                                                       │
     │                                                                       ▼
     └─────────────── Restored Response ◀─── Sidecar ◀─── Provider Response │
```

**Critical Invariant**: At no point does original PII cross the trust boundary to external providers.

## Compliance Notes

This plugin provides technical controls for PII protection but **does not guarantee GDPR/CCPA/HIPAA compliance**. Compliance requires:

- Organizational policies and procedures
- Data processing agreements with providers
- Regular audits and monitoring
- Incident response procedures
- Legal review of data flows

The plugin provides the *technical enforcement* of the privacy boundary; organizational controls must complete the compliance posture.