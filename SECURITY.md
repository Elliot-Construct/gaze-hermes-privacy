# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

Please report security vulnerabilities by emailing security@example.com.

We will acknowledge receipt within 48 hours and provide a timeline for a fix.

## Threat Model

This plugin provides reversible PII protection for external LLM calls. The trust boundary is:

- **Trusted**: Local Hermes host, local Gaze sidecar, local trusted providers
- **Untrusted**: External LLM providers (OpenRouter, Anthropic, OpenAI, etc.)

Installing this plugin alone does NOT make a deployment GDPR compliant. It is one layer in a defense-in-depth strategy. The operator is responsible for:

- Securing the Hermes host and sidecar
- Managing secret files (API tokens, snapshot keys)
- Configuring trusted providers correctly
- Monitoring audit logs for anomalies

## Out of Scope

- Host compromise (root access to Hermes machine)
- Sidecar binary tampering
- Network interception between Hermes and sidecar
- Vulnerabilities in the Gaze library itself
- Vulnerabilities in Hermes core

## Failure-Closed Guarantees

When the sidecar is unavailable or Hermes lacks required capabilities:

- **Mandatory mode** (default): All external provider calls are blocked
- **Compatibility mode**: External calls proceed but are marked "protection not guaranteed"
- **Trusted-local providers**: Always bypass, regardless of sidecar state

## Secrets Management

- API tokens and snapshot keys are stored in files with 0600 permissions
- Generated secrets use cryptographically secure random bytes (32 bytes)
- Operator-supplied secrets are never overwritten
- Docker mode mounts secrets read-only