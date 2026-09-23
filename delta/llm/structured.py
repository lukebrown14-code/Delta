"""JSON-schema enforced calls returning validated Pydantic objects."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path
from typing import Any

from pydantic import BaseModel, TypeAdapter, ValidationError

from delta.llm.client import LLMClient, LLMResult
from delta.llm.json import extract_json

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


def _cache_validator(adapter: TypeAdapter[Any]) -> Callable[[str], bool]:
    """True when ``text`` parses as the schema and validates against it.

    Used to gate the cache: a response that fails this is not a usable hit, so
    an earlier bad answer is recalled live instead of replaying the same
    failure every call (the poisoned-JSON loop).
    """

    def valid(text: str) -> bool:
        try:
            adapter.validate_python(extract_json(text))
            return True
        except (ValidationError, ValueError):
            return False

    return valid


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
    validated object and the ``LLMResult`` (call id, cost, cache flag). Cached
    responses are only reused when they still validate, so a bad cached answer
    is not re-served.
    """
    prompt = render_prompt(template, vars)
    prompt_version = template.removesuffix(".j2")
    response_format = {
        "type": "json_schema",
        "json_schema": {"name": schema.__name__, "schema": schema_from_model(schema)},
    }
    adapter: TypeAdapter[Any] = TypeAdapter(schema)
    validator = _cache_validator(adapter)

    result = await client.complete(
        task=task,
        model=model,
        prompt_version=prompt_version,
        prompt=prompt,
        response_format=response_format,
        cache_validator=validator,
    )

    parsed = _parse_or_empty(result.text)
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
            cache_validator=validator,
        )
        parsed2 = _parse_or_empty(result2.text)
        return adapter.validate_python(parsed2), result2


def _parse_or_empty(text: str) -> Any:
    """Parse JSON, or return ``{}`` so schema validation reports a clean error."""
    try:
        return extract_json(text)
    except ValueError:
        return {}
