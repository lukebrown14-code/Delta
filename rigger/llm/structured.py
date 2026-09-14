"""JSON-schema enforced calls returning validated Pydantic objects."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from pydantic import BaseModel, TypeAdapter, ValidationError

from rigger.llm.client import LLMClient, LLMResult

PROMPTS_DIR = Path(__file__).parent / "prompts"

try:
    from jinja2 import Environment, FileSystemLoader, StrictUndefined

    _env: Environment | None = Environment(
        loader=FileSystemLoader(PROMPTS_DIR),
        undefined=StrictUndefined,
        autoescape=False,
    )
except ImportError:  # pragma: no cover
    _env = None


def render_prompt(template: str, vars: dict[str, Any]) -> str:
    if _env is None:
        raise RuntimeError("jinja2 is required")
    tpl = _env.get_template(template)
    return tpl.render(**vars)


def schema_from_model(model: type[BaseModel]) -> dict[str, Any]:
    return model.model_json_schema()


async def structured[T: BaseModel](
    client: LLMClient,
    *,
    task: str,
    model: str,
    template: str,
    vars: dict[str, Any],
    schema: type[T],
) -> tuple[T, LLMResult]:
    """Render the template, call the model with a JSON schema, validate the result.

    On validation failure, re-prompts once with the error appended. Returns the
    validated object and the ``LLMResult`` (call id, cost, cache flag).
    """
    prompt = render_prompt(template, vars)
    prompt_version = template.removesuffix(".j2")
    response_format = {
        "type": "json_schema",
        "json_schema": {"name": schema.__name__, "schema": schema_from_model(schema)},
    }

    result = await client.complete(
        task=task,
        model=model,
        prompt_version=prompt_version,
        prompt=prompt,
        response_format=response_format,
    )

    parsed = _extract_json(result.text)
    adapter = TypeAdapter(schema)
    try:
        obj = adapter.validate_python(parsed)
        return obj, result
    except ValidationError as exc:
        retry_prompt = (
            prompt + "\n\nYour previous answer was invalid JSON. Fix the following errors and "
            "return valid JSON only:\n" + str(exc)
        )
        result2 = await client.complete(
            task=task,
            model=model,
            prompt_version=prompt_version,
            prompt=retry_prompt,
            response_format=response_format,
        )
        parsed2 = _extract_json(result2.text)
        return adapter.validate_python(parsed2), result2


def _extract_json(text: str) -> Any:
    text = text.strip()
    if text.startswith("```"):
        text = text.split("```", 2)[1]
        if text.startswith("json"):
            text = text[4:]
        text = text.strip()
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return {}
