"""Bedrock Converse API request/response adapters."""

from __future__ import annotations

from gaze_privacy.adapters.common import (
    PreparedPayload,
    TextField,
    UnsupportedCarrierError,
)


def extract_bedrock_converse(payload: dict, mandatory: bool) -> PreparedPayload:
    fields = []
    _extract_bedrock_messages(payload.get("messages", []), "/messages", fields, mandatory)
    return PreparedPayload(payload=payload, fields=fields)


def _extract_bedrock_messages(messages: list, base_path: str, fields: list[TextField], mandatory: bool) -> None:
    for i, msg in enumerate(messages):
        content = msg.get("content", [])
        for j, block in enumerate(content):
            if isinstance(block, dict) and "text" in block:
                fields.append(TextField(path=f"{base_path}/{i}/content/{j}/text", text=block["text"]))
            elif mandatory:
                raise UnsupportedCarrierError(f"Unsupported Bedrock content block: {block}")


def extract_bedrock_converse_response(response: dict) -> PreparedPayload:
    fields = []
    output = response.get("output", {})
    message = output.get("message", {})
    content = message.get("content", [])
    for i, block in enumerate(content):
        if isinstance(block, dict) and "text" in block:
            fields.append(TextField(path=f"/output/message/content/{i}/text", text=block["text"]))
    return PreparedPayload(payload=response, fields=fields)