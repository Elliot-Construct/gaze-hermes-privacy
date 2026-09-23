"""Codex/Responses API request/response adapters."""

from __future__ import annotations

from gaze_privacy.adapters.common import (
    PreparedPayload,
    TextField,
    UnsupportedCarrierError,
)


def extract_codex_responses(payload: dict, mandatory: bool) -> PreparedPayload:
    fields = []
    if "instructions" in payload and isinstance(payload["instructions"], str):
        fields.append(TextField(path="/instructions", text=payload["instructions"]))
    _extract_responses_input(payload.get("input", []), "/input", fields, mandatory)
    return PreparedPayload(payload=payload, fields=fields)


def _extract_responses_input(items: list, base_path: str, fields: list[TextField], mandatory: bool) -> None:
    for i, item in enumerate(items):
        content = item.get("content")
        if content is None:
            continue
        if isinstance(content, list):
            for j, block in enumerate(content):
                if block.get("type") == "input_text":
                    fields.append(TextField(path=f"{base_path}/{i}/content/{j}/text", text=block["text"]))
                elif mandatory:
                    raise UnsupportedCarrierError(f"Unsupported Responses input type: {block.get('type')}")
        elif mandatory:
            raise UnsupportedCarrierError(f"Unsupported input content type: {type(content)}")


def extract_codex_responses_response(response: dict) -> PreparedPayload:
    fields = []
    for i, item in enumerate(response.get("output", [])):
        if item.get("type") == "message":
            content = item.get("content", [])
            for j, block in enumerate(content):
                if block.get("type") == "output_text":
                    fields.append(TextField(path=f"/output/{i}/content/{j}/text", text=block["text"]))
        elif item.get("type") == "function_call":
            args = item.get("arguments")
            if isinstance(args, str):
                fields.append(TextField(path=f"/output/{i}/arguments", text=args))
    return PreparedPayload(payload=response, fields=fields)