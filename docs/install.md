# Installation Guide

## Native-First Installation (Recommended)

### Prerequisites

- Python 3.11+
- Rust 1.89+ (for building sidecar)
- Hermes 0.8.0+

### Steps

1. **Clone and build sidecar**:
   ```bash
   cd gaze-hermes-privacy
   cargo build --release --manifest-path sidecar/Cargo.toml
   ```

2. **Install Python package**:
   ```bash
   pip install -e .
   ```

3. **Configure**:
   ```bash
   mkdir -p ~/.hermes/gaze-hermes-privacy/{secrets,policies/profiles}
   cp config.example.toml ~/.hermes/gaze-hermes-privacy/config.toml
   ```

3. **Enable plugin**:
   ```bash
   hermes plugin enable gaze-hermes-privacy
   ```

## Docker Mode

### Prerequisites

- Docker 24.0+
- Docker Compose v2

### Steps

1. **Build image**:
   ```bash
   docker compose build
   ```

2. **Start sidecar**:
   ```bash
   docker compose up -d
   ```

3. **Configure Hermes** to use `http://127.0.0.1:65113` as sidecar endpoint.

4. **Set plugin config**:
   ```toml
   [sidecar]
   mode = "docker"
   url = "http://127.0.0.1:65113"
   ```

## External Sidecar Mode

For running the sidecar on a separate machine:

1. **Deploy sidecar** on target machine (native or Docker)
2. **Configure Hermes** to point to remote sidecar:
   ```toml
   [sidecar]
   mode = "external"
   url = "http://sidecar-host:65113"
   ```

## NER First-Run Download

On first run with NER enabled, the sidecar automatically downloads the Kiji DistilBERT model (~50MB):

```bash
# Automatic on first sidecar start with NER policy
# Model stored in: ~/.hermes/gaze-hermes-privacy/models/kiji-distilbert/
```

To pre-download:
```bash
python -c "
from gaze_privacy.sidecar_manager import SidecarManager
from gaze_privacy.config import PrivacyConfig
import asyncio

cfg = PrivacyConfig.load()
mgr = SidecarManager(cfg)
asyncio.run(mgr.ensure_running('default'))
"
```

## Offline Model Provisioning

For air-gapped environments:

1. **Download model** on connected machine:
   ```bash
   python -c "
   from gaze_model_setup import install_kiji_bundle, InstallOptions, KijiDistilbertPrecision
   opts = InstallOptions(model_dir=Path('/tmp/kiji-distilbert'), precision=KijiDistilbertPrecision.F32)
   install_kiji_bundle(opts)
   ```

2. **Copy model directory** to target machine

3. **Configure policy** to use local model:
   ```toml
   [policy.ner]
   model_dir = "/path/to/kiji-distilbert"
   ```

## Secret Files and Generated Fallbacks

The plugin manages two secret files:

| File | Purpose | Generated Fallback |
|------|---------|-------------------|
| `secrets/api-token` | Sidecar Bearer token | 32-byte URL-safe random |
| `secrets/snapshot-key` | Session encryption key | 32-byte URL-safe random |

- **Operator-supplied**: If file exists, it's used as-is (never overwritten)
- **Generated**: Created with `os.open(O_EXCL, 0o600)` for atomic creation with 0600 permissions
- **Location**: `~/.hermes/gaze-hermes-privacy/secrets/` by default, configurable in `config.toml`

## Configuration Reference

```toml
[sidecar]
mode = "native"      # native | docker | external
scope = "host"       # host | profile
url = "http://127.0.0.1:65113"  # external mode only

[providers]
trusted_local = ["local-vllm"]  # exact provider IDs only

[security]
mandatory = true           # block external if sidecar unavailable
compatibility_mode = false # allow external with warning if capabilities missing

[policy]
global_file = "policies/global.toml"
profile_dir = "policies/profiles"
```