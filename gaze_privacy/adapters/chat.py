"""Chat completions and Anthropic Messages request/response adapters."""

from __future__ import annotations

from gaze_privacy.adapters.common import (
    PreparedPayload,
    TextField,
    UnsupportedCarrierError,
)


def extract_request_fields(api_mode: str, payload: dict, mandatory: bool = True) -> PreparedPayload:
    if api_mode == "chat_completions":
        return _extract_chat_completions(payload, mandatory)
    elif api_mode == "anthropic_messages":
        return _extract_anthropic_messages(payload, mandatory)
    elif api_mode == "codex_responses":
        from gaze_privacy.adapters.responses import extract_codex_responses
        return extract_codex_responses(payload, mandatory)
    elif api_mode == "bedrock_converse":
        from gaze_privacy.adapters.bedrock import extract_bedrock_converse
        return extract_bedrock_converse(payload, mandatory)
    else:
        raise ValueError(f"Unsupported api_mode: {api_mode}")


def _extract_chat_completions(payload: dict, mandatory: bool) -> PreparedPayload:
    fields = []
    _extract_chat_messages(payload.get("messages", []), "/messages", fields, mandatory)
    # Check for unsupported top-level fields that may contain user text
    if mandatory and "input" in payload:
        # The Responses API uses "input" - reject if present in chat_completions
        for i, item in enumerate(payload.get("input", [])):
            if item.get("type") in ("input_audio", "input_text"):
                if item.get("type") == "input_audio":
                    raise UnsupportedCarrierError(f"Unsupported content type: {item.get('type')}")
    return PreparedPayload(payload=payload, fields=fields)


def _extract_chat_messages(messages: list, base_path: str, fields: list[TextField], mandatory: bool) -> None:
    for i, msg in enumerate(messages):
        content = msg.get("content")
        if content is None:
            continue
        if isinstance(content, str):
            fields.append(TextField(path=f"{base_path}/{i}/content", text=content))
        elif isinstance(content, list):
            for j, part in enumerate(content):
                if part.get("type") == "text":
                    fields.append(TextField(path=f"{base_path}/{i}/content/{j}/text", text=part["text"]))
                elif part.get("type") in ("image_url", "input_audio", "unknown_blob"):
                    if mandatory:
                        raise UnsupportedCarrierError(f"Unsupported content type: {part.get('type')}")
        elif mandatory:
            raise UnsupportedCarrierError(f"Unsupported content type: {type(content)}")


def _extract_anthropic_messages(payload: dict, mandatory: bool) -> PreparedPayload:
    fields = []
    if "system" in payload and isinstance(payload["system"], str):
        fields.append(TextField(path="/system", text=payload["system"]))
    _extract_anthropic_content_blocks(payload.get("messages", []), "/messages", fields, mandatory)
    return PreparedPayload(payload=payload, fields=fields)


def _extract_anthropic_content_blocks(messages: list, base_path: str, fields: list[TextField], mandatory: bool) -> None:
    for i, msg in enumerate(messages):
        content = msg.get("content")
        if content is None:
            continue
        if isinstance(content, str):
            fields.append(TextField(path=f"{base_path}/{i}/content", text=content))
        elif isinstance(content, list):
            for j, block in enumerate(content):
                if block.get("type") == "text":
                    fields.append(TextField(path=f"{base_path}/{i}/content/{j}/text", text=block["text"]))
                elif mandatory:
                    raise UnsupportedCarrierError(f"Unsupported Anthropic content type: {block.get('type')}")
        elif mandatory:
            raise UnsupportedCarrierError(f"Unsupported content type: {type(content)}")


def extract_response_fields(api_mode: str, response: dict) -> PreparedPayload:
    if api_mode == "chat_completions":
        return _extract_chat_completions_response(response)
    elif api_mode == "anthropic_messages":
        return _extract_anthropic_messages_response(response)
    elif api_mode == "codex_responses":
        from gaze_privacy.adapters.responses import extract_codex_responses_response
        return extract_codex_responses_response(response)
    elif api_mode == "bedrock_converse":
        from gaze_privacy.adapters.bedrock import extract_bedrock_converse_response
        return extract_bedrock_converse_response(response)
    else:
        raise ValueError(f"Unsupported api_mode: {api_mode}")


def _extract_chat_completions_response(response: dict) -> PreparedPayload:
    fields = []
    for idx, choice in enumerate(response.get("choices", [])):
        msg = choice.get("message", {})
        content = msg.get("content")
        if isinstance(content, str):
            fields.append(TextField(path=f"/choices/{idx}/message/content", text=content))
        # tool_calls arguments
        for tc_idx, tc in enumerate(msg.get("tool_calls", [])):
            func = tc.get("function", {})
            args = func.get("arguments")
            if isinstance(args, str):
                fields.append(TextField(
                    path=f"/choices/{idx}/message/tool_calls/{tc_idx}/function/arguments",
                    text=args,
                ))
    return PreparedPayload(payload=response, fields=fields)


def _extract_anthropic_messages_response(response: dict) -> PreparedPayload:
    fields = []
    content = response.get("content", [])
    for i, block in enumerate(content):
        if block.get("type") == "text":
            fields.append(TextField(path=f"/content/{i}/text", text=block["text"]))
    return PreparedPayload(payload=response, fields=fields)


def restore_completed_response(client, namespace, api_mode: str, response: dict) -> dict:
    """Restore a completed provider response using the sidecar client."""
    prepared = extract_response_fields(api_mode, response)
    if not prepared.fields:
        return response
    restored = client.restore(namespace, prepared.fields)
    return prepared.apply(restored["fields"])