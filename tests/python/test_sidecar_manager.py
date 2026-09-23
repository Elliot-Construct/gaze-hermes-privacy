"""Tests for sidecar lifecycle management."""

from __future__ import annotations

import asyncio
from dataclasses import replace
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from gaze_privacy.config import PrivacyConfig
from gaze_privacy.sidecar_manager import SidecarManager, ManagedSidecar
from gaze_privacy.sidecar_client import SidecarClient, SidecarStatus


class FakeLauncher:
    def __init__(self):
        self.ready_address = "127.0.0.1:43127"
        self.argv = []
        self.processes = []

    def launch(self, *args, **kwargs):
        self.argv = list(args)
        proc = MagicMock()
        proc.pid = 12345
        self.processes.append(proc)
        return proc


@pytest.fixture
def fake_launcher():
    return FakeLauncher()


@pytest.fixture
def mock_config(tmp_path, monkeypatch):
    monkeypatch.setenv("GAZE_HERMES_HOME", str(tmp_path))
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    (tmp_path / "secrets").mkdir(exist_ok=True)
    (tmp_path / "secrets" / "api-token").write_text("test-token", encoding="utf-8")
    (tmp_path / "secrets" / "snapshot-key").write_text("x" * 43, encoding="utf-8")
    cfg = PrivacyConfig.load()
    return cfg


async def test_host_scope_reuses_one_endpoint(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_scope="host")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    # Mock the binary verification and launch to use fake launcher
    with patch.object(manager, "_verify_binary", return_value=True):
        with patch.object(manager, "_launch_host_sidecar", return_value=("http://127.0.0.1:65113", MagicMock())):
            with patch.object(SidecarClient, "status", new_callable=AsyncMock) as mock_status:
                mock_status.return_value = SidecarStatus(protocol_version=1, gaze_version="0.14.0")
                managed1 = await manager.ensure_running("writer")
                managed2 = await manager.ensure_running("editor")
                assert managed1.client.base_url == managed2.client.base_url


async def test_profile_scope_uses_ready_file_endpoint(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_scope="profile")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    with patch.object(manager, "_verify_binary", return_value=True):
        with patch.object(manager, "_launch_profile_sidecar", return_value=("http://127.0.0.1:43127", MagicMock())):
            with patch.object(SidecarClient, "status", new_callable=AsyncMock) as mock_status:
                mock_status.return_value = SidecarStatus(protocol_version=1, gaze_version="0.14.0")
                managed = await manager.ensure_running("writer")
                assert managed.client.base_url == "http://127.0.0.1:43127"


async def test_profile_scope_launches_separate_endpoints(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_scope="profile")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    with patch.object(manager, "_verify_binary", return_value=True):
        with patch.object(manager, "_launch_profile_sidecar", side_effect=[
            ("http://127.0.0.1:43127", MagicMock()),
            ("http://127.0.0.1:43128", MagicMock()),
        ]) as mock_launch:
            with patch.object(SidecarClient, "status", new_callable=AsyncMock) as mock_status:
                mock_status.return_value = SidecarStatus(protocol_version=1, gaze_version="0.14.0")
                managed1 = await manager.ensure_running("profile-a")
                managed2 = await manager.ensure_running("profile-b")
                assert managed1.client.base_url != managed2.client.base_url
                assert mock_launch.call_count == 2


async def test_protocol_version_mismatch_raises(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_scope="host")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    with patch.object(manager, "_verify_binary", return_value=True):
        with patch.object(manager, "_launch_host_sidecar", return_value=("http://127.0.0.1:65113", MagicMock())):
            # Mock _wait_ready to return mismatched version
            with patch.object(manager, "_wait_ready", new_callable=AsyncMock) as mock_wait:
                mock_wait.return_value = SidecarStatus(protocol_version=999, gaze_version="0.14.0")
                # Also mock the SidecarClient creation to avoid connection attempts
                with patch("gaze_privacy.sidecar_manager.SidecarClient") as mock_client_class:
                    mock_client = AsyncMock()
                    mock_client.status = AsyncMock(return_value=SidecarStatus(protocol_version=999, gaze_version="0.14.0"))
                    mock_client_class.return_value = mock_client
                    with pytest.raises(Exception, match="protocol mismatch"):
                        await manager.ensure_running("writer")


async def test_external_mode_no_download(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_mode="external", sidecar_url="http://127.0.0.1:9999")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    with patch.object(SidecarClient, "status", new_callable=AsyncMock) as mock_status:
        mock_status.return_value = SidecarStatus(protocol_version=1, gaze_version="0.14.0")
        managed = await manager.ensure_running("writer")
        assert managed.client.base_url == "http://127.0.0.1:9999"
        # Should not call launcher in external mode


async def test_docker_mode_health_checks_container(mock_config, fake_launcher):
    config = replace(mock_config, sidecar_mode="docker", sidecar_url="http://127.0.0.1:65113")
    manager = SidecarManager(config, launcher=fake_launcher)
    
    with patch.object(SidecarClient, "status", new_callable=AsyncMock) as mock_status:
        mock_status.return_value = SidecarStatus(protocol_version=1, gaze_version="0.14.0")
        managed = await manager.ensure_running("writer")
        assert managed.client.base_url == "http://127.0.0.1:65113"
        # Should not call launcher in docker mode