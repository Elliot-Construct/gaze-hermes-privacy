"""Tests for response field extraction adapters."""

from __future__ import annotations

import pytest

from gaze_privacy.adapters import (
    extract_response_fields,
    restore_completed_response,
    TextField,
)


class FakeClient:
    def __init__(self, mapping: dict[str, str]):
        self.mapping = mapping

    def restore(self, namespace, fields):
        result = {"fields": []}
        for f in fields:
            restored_text = f.text
            for token, replacement in self.mapping.items():
                restored_text = restored_text.replace(token, replacement)
            result["fields"].append(TextField(path=f.path, text=restored_text))
        return result


def test_chat_completions_response_extracts_content():
    response = {
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello <token> there",
                },
                "finish_reason": "stop",
            }
        ],
    }
    prepared = extract_response_fields("chat_completions", response)
    paths = [f.path for f in prepared.fields]
    assert "/choices/0/message/content" in paths
    assert prepared.payload["id"] == "chatcmpl-123"
    assert prepared.payload["model"] == "gpt-4"
    # Structural fields preserved
    assert prepared.payload["choices"][0]["message"]["role"] == "assistant"
    assert prepared.payload["choices"][0]["finish_reason"] == "stop"


def test_chat_completions_response_with_tool_calls():
    response = {
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "model": "gpt-4",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": None,
                    "tool_calls": [
                        {
                            "id": "call_123",
                            "type": "function",
                            "function": {
                                "name": "write_file",
                                "arguments": '{"path": "letter.tex", "content": "Dear <token>,\\n"}',
                            },
                        }
                    ],
                },
                "finish_reason": "tool_calls",
            }
        ],
    }
    prepared = extract_response_fields("chat_completions", response)
    paths = [f.path for f in prepared.fields]
    assert "/choices/0/message/tool_calls/0/function/arguments" in paths
    # Other fields preserved
    assert prepared.payload["choices"][0]["message"]["role"] == "assistant"
    assert prepared.payload["choices"][0]["finish_reason"] == "tool_calls"


def test_anthropic_messages_response_extracts_content():
    response = {
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "model": "claude-3",
        "content": [
            {"type": "text", "text": "Hello <token> there"},
            {"type": "text", "text": "More text"},
        ],
        "stop_reason": "end_turn",
    }
    prepared = extract_response_fields("anthropic_messages", response)
    paths = [f.path for f in prepared.fields]
    assert "/content/0/text" in paths
    assert "/content/1/text" in paths
    # Structural preserved
    assert prepared.payload["id"] == "msg_123"
    assert prepared.payload["role"] == "assistant"
    assert prepared.payload["stop_reason"] == "end_turn"


def test_bedrock_converse_response_extracts_content():
    response = {
        "output": {
            "message": {
                "role": "assistant",
                "content": [{"text": "Hello <token> there"}],
            }
        },
        "stopReason": "end_turn",
    }
    prepared = extract_response_fields("bedrock_converse", response)
    paths = [f.path for f in prepared.fields]
    assert "/output/message/content/0/text" in paths
    # Structural preserved
    assert prepared.payload["output"]["message"]["role"] == "assistant"
    assert prepared.payload["stopReason"] == "end_turn"


def test_codex_responses_response_extracts_output_text():
    response = {
        "id": "resp_123",
        "model": "gpt-4",
        "output": [
            {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Hello <token>"}]},
            {"type": "function_call", "call_id": "call_123", "name": "write_file", "arguments": '{"content": "Dear <token>"}'},
        ],
    }
    prepared = extract_response_fields("codex_responses", response)
    paths = [f.path for f in prepared.fields]
    assert "/output/0/content/0/text" in paths
    assert "/output/1/arguments" in paths
    # Structural preserved
    assert prepared.payload["id"] == "resp_123"
    assert prepared.payload["output"][0]["role"] == "assistant"
    assert prepared.payload["output"][0]["type"] == "message"
    assert prepared.payload["output"][1]["name"] == "write_file"
    assert prepared.payload["output"][1]["call_id"] == "call_123"


def test_restore_completed_response_chat_completions():
    client = FakeClient({"Hello <token> there": "Hello alice@example.invalid there"})
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    response = {
        "choices": [{"message": {"content": "Hello <token> there"}}],
    }
    result = restore_completed_response(client, namespace, "chat_completions", response)
    assert result["choices"][0]["message"]["content"] == "Hello alice@example.invalid there"


def test_restore_completed_response_preserves_structure():
    client = FakeClient({"<token>": "alice@example.invalid"})
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    response = {
        "id": "chatcmpl-123",
        "model": "gpt-4",
        "choices": [
            {"message": {"role": "assistant", "content": "Contact <token>"}}
        ],
    }
    result = restore_completed_response(client, namespace, "chat_completions", response)
    assert result["id"] == "chatcmpl-123"
    assert result["model"] == "gpt-4"
    assert result["choices"][0]["message"]["role"] == "assistant"
    assert result["choices"][0]["message"]["content"] == "Contact alice@example.invalid"


def test_restore_completed_response_tool_call_arguments():
    client = FakeClient({'{"content": "Dear <token>"}': '{"content": "Dear alice@example.invalid"}'})
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    response = {
        "choices": [
            {
                "message": {
                    "tool_calls": [
                        {
                            "function": {
                                "arguments": '{"content": "Dear <token>"}',
                            }
                        }
                    ]
                }
            }
        ],
    }
    result = restore_completed_response(client, namespace, "chat_completions", response)
    assert result["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] == '{"content": "Dear alice@example.invalid"}'


def test_restore_no_fields_returns_original():
    client = FakeClient({})
    namespace = {"profile_id": "default", "session_id": "s1", "request_id": "r1"}
    response = {"id": "chatcmpl-123", "choices": [{"message": {"content": "No tokens here"}}]}
    # Mock extract_response_fields to return no fields
    import gaze_privacy.adapters.chat as chat_module
    original = chat_module.extract_response_fields
    chat_module.extract_response_fields = lambda *args, **kwargs: type("Prepared", (), {"fields": [], "payload": response})()
    try:
        result = restore_completed_response(client, namespace, "chat_completions", response)
        assert result is response
    finally:
        chat_module.extract_response_fields = original