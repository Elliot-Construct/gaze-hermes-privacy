# Gaze Hermes Privacy

Reversible PII protection for Hermes external LLM calls via a local Gaze sidecar.

## Install in Hermes

1. Clone this repository into your Hermes plugins directory:
   ```bash
   cd ~/.hermes/plugins
   git clone https://github.com/example/gaze-hermes-privacy
   ```

2. Install Python dependencies:
   ```bash
   cd gaze-hermes-privacy
   pip install -e .[dev]
   ```

3. Enable the plugin in Hermes:
   ```bash
   hermes plugin enable gaze-hermes-privacy
   ```

## How protection works

The plugin intercepts outbound LLM requests and inbound responses through Hermes middleware:

1. **Request cleaning**: Text fields are extracted from the request payload and sent to the local Gaze sidecar for tokenization. PII is replaced with reversible tokens (e.g., `<token:email:1>`).

2. **Provider call**: The cleaned request is sent to the external LLM provider. The provider never sees original PII.

3. **Response restoration**: The provider's response is passed through the sidecar to restore tokens to original values before Hermes processes tool calls or delivers to the user.

4. **Streaming restoration**: For streaming responses, tokens are restored in real-time as chunks arrive, ensuring TTS and UI display show real values.

## Trusted local providers

Providers explicitly listed in `trusted_local` bypass all protection:

```toml
[providers]
trusted_local = ["local-vllm", "ollama"]
```

Only exact provider ID matches are trusted. No hostname or URL inference is performed.

## Desktop privacy console

The Hermes Desktop plugin provides:

- **Overview**: Protection state, sidecar version, policy hash, counters
- **Live Debug**: Real-time event stream with detections and decisions
- **Rules**: Visual + Advanced TOML policy editor with edit/validate/apply flow
- **Test Lab**: Local policy testing without calling LLM providers
- **Providers**: Trust management with explicit confirmation
- **Sessions**: Session management with recover/reset/delete
- **Reveal**: Temporary (60s) sensitive data reveal for debugging

## Compatibility and Hermes version

| Hermes version | Privacy support |
|----------------|-----------------|
| < 0.8.0        | Not supported (no middleware API) |
| 0.8.0+         | Full support with `llm_execution` middleware |
| 0.9.0+         | Full support with `llm_stream_text` middleware |

On unsupported Hermes versions:
- **Mandatory mode** (default): Blocks all external providers
- **Compatibility mode**: Allows external calls but marks them "protection not guaranteed"