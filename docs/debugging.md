# Debugging Guide

## Health and Protocol Checks

### Sidecar Health

```bash
# Quick health check (unauthenticated)
curl http://127.0.0.1:65113/healthz

# Expected: "OK"
```

### Sidecar Status

```bash
# With authentication
curl -H "Authorization: Bearer <token>" http://127.0.0.1:65113/v1/status

# Expected JSON:
# {
#   "protocol_version": 1,
#   "gaze_version": "0.14.0",
#   "ner_model": "kiji-distilbert"
# }
```

### Protocol Version Mismatch

If you see `protocol_version` mismatch:
- Ensure sidecar and plugin are built from same commit
- Check `sidecar-release.json` protocol_version matches

## Blocked External Request Diagnostics

### Mandatory Mode Blocks

When mandatory mode is enabled and sidecar is unavailable:

```bash
# Check plugin logs
grep "PrivacyBlockedError" /var/log/hermes/plugin.log

# Verify sidecar is running
systemctl status gaze-hermes-sidecar
# or
docker ps | gaze-hermes-sidecar
```

### Compatibility Mode Warnings

When compatibility mode is enabled:
```bash
grep "protection_not_guaranteed" /var/log/hermes/plugin.log
```

### Trusted-Local Verification

```bash
# Check provider policy
grep "trusted_local" ~/.hermes/gaze-hermes-privacy/config.toml

# Verify exact match behavior
grep -i "local-vllm" /var/log/hermes/plugin.log
```

## Policy Validation

### Validate Policy Syntax

```bash
# Via sidecar API
curl -H "Authorization: Bearer <token>" \
  -X POST http://127.0.0.1:65113/v1/policies/validate \
  -H "Content-Type: application/json" \
  -d '{"toml": "<your policy toml>"}'
```

### Test Policy Before Apply

```bash
# Test with sample text
curl -H "Authorization: Bearer <token>" \
  -X POST http://127.0.0.1:65113/v1/policies/test \
  -H "Content-Type: application/json" \
  -d '{"scope": "global", "toml": "<policy>", "sample": "Contact alice@example.invalid"}'
```

### Common Validation Errors

| Error | Cause | Fix |
|-------|-------|-----|
| `UnknownClass` | Class not in rulepack | Add custom recognizer or use valid class |
| `BadRegex` | Invalid regex in custom recognizer | Fix regex syntax |
| `NoDetectors` | No detectors and no rulepacks | Add `bundled = ["core"]` or custom detectors |
| `NerThresholdOutOfRange` | NER threshold not in 0..1 | Set threshold between 0.0 and 1.0 |
| `PolicySchemaUnsupported` | Schema version not "0.1.x" | Update to supported schema |

## Snapshot Recovery

### Failed Recovery

If sidecar fails to recover session:

```bash
# Check snapshot file exists
ls -la ~/.hermes/gaze-hermes-privacy/snapshots/

# Verify master key matches
sha256sum ~/.hermes/gaze-hermes-privacy/secrets/snapshot-key
```

### Corrupt Snapshot

If snapshot is corrupted (wrong key or tampered):

```bash
# Delete corrupt snapshot to allow fresh session
rm ~/.hermes/gaze-hermes-privacy/snapshots/<profile>_<session>.enc

# Or use API
curl -X DELETE -H "Authorization: Bearer <token>" \
  http://127.0.0.1:65113/v1/sessions/<profile>/<session>
```

## Sensitive Reveal Controls

### Reveal Not Working

- Ensure event exists and hasn't expired (60s TTL)
- Check profile matches event's profile
- Verify grant hasn't been consumed

### Reveal Not Clearing

- Switch profile to trigger auto-clear
- Navigate away from Events tab
- Disconnect backend to trigger clear

## Common Issues

| Symptom | Likely Cause | Resolution |
|---------|--------------|------------|
| Sidecar won't start | Port 65113 in use | `lsof -i :65113` and kill |
| Policy apply returns 409 | Stale expected_hash | GET `/policies/effective` for current hash |
| NER not working | Model not downloaded | Check sidecar logs for download errors |
| Docker sidecar fails | Volume mount permissions | Check bind mount paths and permissions |
| WebSocket disconnects | Network proxy/timeout | Check firewall, increase timeout |
| Python import errors | Package not installed | `pip install -e .[dev]` in plugin dir |

## Log Locations

| Component | Location |
|-----------|----------|
| Hermes plugin | `~/.hermes/logs/plugin-gaze-hermes-privacy.log` |
| Sidecar (native) | `journalctl -u gaze-hermes-sidecar` |
| Sidecar (Docker) | `docker logs gaze-hermes-sidecar` |
| Desktop plugin | Hermes DevTools console |

## Forbidden Fixture Scan

To ensure no real PII in tests:
```bash
grep -r "alice@example\.com\|bob@example\.com\|real.*email\|real.*phone" tests/ --include="*.py" --include="*.json"
```

Should return no matches.