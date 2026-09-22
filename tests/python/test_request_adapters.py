"""Tests for request field extraction adapters."""

from __future__ import annotations

import pytest

from gaze_privacy.adapters import (
    extract_request_fields,
    UnsupportedCarrierError,
    TextField,
)


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


def test_chat_completions_tool_results_are_protected():
    payload = {
        "model": "m",
        "messages": [
            {"role": "user", "content": "Call tool"},
            {"role": "tool", "tool_call_id": "call_123", "content": "Result: alice@example.invalid"},
        ],
    }
    prepared = extract_request_fields("chat_completions", payload, mandatory=True)
    paths = [f.path for f in prepared.fields]
    assert "/messages/0/content" in paths
    assert "/messages/1/content" in paths
    # tool_call_id, role preserved
    assert prepared.payload["messages"][1]["tool_call_id"] == "call_123"
    assert prepared.payload["messages"][1]["role"] == "tool"


def test_chat_completions_tool_definitions_preserved():
    payload = {
        "model": "m",
        "messages": [{"role": "user", "content": "Hi"}],
        "tools": [
            {"type": "function", "function": {"name": "send_email", "description": "Send email to user@example.invalid"}}
        ],
    }
    prepared = extract_request_fields("chat_completions", payload, mandatory=True)
    # tool description is structural, not user text
    assert prepared.payload["tools"][0]["function"]["description"] == "Send email to user@example.invalid"
    # Only user message content extracted
    assert [f.path for f in prepared.fields] == ["/messages/0/content"]


def test_chat_completions_json_schema_preserved():
    payload = {
        "model": "m",
        "messages": [{"role": "user", "content": "Hi"}],
        "tools": [
            {
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "location": {"type": "string", "description": "City like New York"}
                        },
                    },
                },
            }
        ],
    }
    prepared = extract_request_fields("chat_completions", payload, mandatory=True)
    assert prepared.payload["tools"][0]["function"]["parameters"]["properties"]["location"]["description"] == "City like New York"


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


def test_non_mandatory_mode_allows_unknown_carriers():
    payload = {"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,AAAA"}}]}]}
    # Should not raise, but may not extract text from opaque parts
    prepared = extract_request_fields("chat_completions", payload, mandatory=False)
    # No fields extracted from opaque content
    assert prepared.fields == []


def test_anthropic_messages_multiple_content_blocks():
    payload = {
        "model": "m",
        "system": "System prompt",
        "messages": [
            {"role": "user", "content": [
                {"type": "text", "text": "Hello"},
                {"type": "text", "text": "World"},
            ]},
            {"role": "assistant", "content": [
                {"type": "text", "text": "Hi there"},
            ]},
        ],
    }
    prepared = extract_request_fields("anthropic_messages", payload, mandatory=True)
    paths = [f.path for f in prepared.fields]
    assert "/system" in paths
    assert "/messages/0/content/0/text" in paths
    assert "/messages/0/content/1/text" in paths
    assert "/messages/1/content/0/text" in paths
    # Role preserved
    assert prepared.payload["messages"][0]["role"] == "user"
    assert prepared.payload["messages"][1]["role"] == "assistant"


def test_codex_responses_instructions_and_input():
    payload = {
        "model": "m",
        "instructions": "You are a helpful assistant in York",
        "input": [
            {"role": "user", "content": [{"type": "input_text", "text": "Contact alice@example.invalid"}]},
            {"role": "assistant", "content": [{"type": "input_text", "text": "Sure thing"}]},
        ],
    }
    prepared = extract_request_fields("codex_responses", payload, mandatory=True)
    paths = [f.path for f in prepared.fields]
    assert "/instructions" in paths
    assert "/input/0/content/0/text" in paths
    assert "/input/1/content/0/text" in paths


def test_bedrock_converse_multiple_messages():
    payload = {
        "modelId": "m",
        "messages": [
            {"role": "user", "content": [{"text": "Hello"}]},
            {"role": "assistant", "content": [{"text": "Hi there"}]},
        ],
    }
    prepared = extract_request_fields("bedrock_converse", payload, mandatory=True)
    paths = [f.path for f in prepared.fields]
    assert "/messages/0/content/0/text" in paths
    assert "/messages/1/content/0/text" in paths
    # modelId preserved
    assert prepared.payload["modelId"] == "m"
    assert prepared.payload["messages"][0]["role"] == "user"