"""Provider-agnostic LLM client: caching, cost + latency logging, retry policy.

The actual model call is delegated to a :class:`delta.llm.providers.Provider`
(OpenRouter, or any OpenAI-compatible endpoint). This client owns the cache
keyed on ``sha256(model + prompt_version + prompt)`` and persists every call to
the ``llmcall`` table for cost tracking and backtest replay.

``complete`` is the single entry point. It accepts either a single ``prompt``
(building a ``[system, user]`` pair) or a full ``messages`` transcript, so both
structured calls and chat calls share one cache/log path. ``chat`` remains as a
backward-compatible alias that forwards a transcript.
"""

from __future__ import annotations

import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from sqlalchemy.engine import Engine

from delta.core.ids import stable_id
from delta.llm.cache import lookup_cache, store_call
from delta.llm.providers import (
    PROVIDERS,
    OpenAICompatProvider,
    OpenRouterProvider,
    Provider,
    ProviderSpec,
)

SYSTEM_PROMPT = "You are an investment analyst."

#: A predicate deciding whether a cached response is still a valid hit. When
#: given, a cached response failing the check is treated as a miss (and never
#: re-served), so a previously-poisoned key is re-attempted live rather than
#: replaying the same failure on every call.
CacheValidator = Callable[[str], bool]


@dataclass
class LLMResult:
    text: str
    call_id: str
    cost_usd: float
    cached: bool


class LLMClient:
    def __init__(self, provider: Provider, engine: Engine) -> None:
        self.provider = provider
        self.engine = engine

    @staticmethod
    def prompt_hash(model: str, prompt_version: str, prompt: str) -> str:
        return stable_id(model, prompt_version, prompt)

    async def complete(
        self,
        *,
        task: str,
        model: str,
        prompt_version: str,
        prompt: str | None = None,
        messages: list[dict[str, str]] | None = None,
        response_format: dict[str, Any] | None = None,
        cache_validator: CacheValidator | None = None,
    ) -> LLMResult:
        """Run one completion, caching the result by prompt hash.

        Either ``prompt`` (a single user turn, wrapped with the system prompt)
        or ``messages`` (the full transcript) is required. Cache hits are
        logged as ``cached=True`` rows so every call — live or replayed — is
        accounted for. When ``cache_validator`` is supplied, a cached response
        that fails it is a miss, so invalid output is never re-served.
        """
        if messages is None:
            messages = [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": prompt or ""},
            ]
            prompt_text = prompt or ""
        else:
            prompt_text = "\n\n".join(
                f"{message['role']}: {message['content']}" for message in messages
            )

        phash = self.prompt_hash(model, prompt_version, prompt_text)

        cached = lookup_cache(self.engine, phash)
        if cached is not None and (
            cache_validator is None or cache_validator(cached.response or "")
        ):
            store_call(
                self.engine,
                task=task,
                model=model,
                prompt_version=prompt_version,
                prompt_hash=phash,
                input_tokens=0,
                output_tokens=0,
                cost_usd=0.0,
                latency_ms=0,
                cached=True,
                response=cached.response,
            )
            return LLMResult(
                text=cached.response or "",
                call_id="",
                cost_usd=0.0,
                cached=True,
            )

        started = time.perf_counter()
        result = await self.provider.complete(
            model=model,
            messages=messages,
            response_format=response_format,
        )
        latency_ms = int((time.perf_counter() - started) * 1000)

        call_id = uuid.uuid4().hex
        store_call(
            self.engine,
            task=task,
            model=model,
            prompt_version=prompt_version,
            prompt_hash=phash,
            input_tokens=result.input_tokens,
            output_tokens=result.output_tokens,
            cost_usd=result.cost_usd,
            latency_ms=latency_ms,
            cached=False,
            response=result.text,
        )
        return LLMResult(
            text=result.text,
            call_id=call_id,
            cost_usd=result.cost_usd,
            cached=False,
        )

    async def chat(
        self,
        *,
        task: str,
        model: str,
        prompt_version: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None = None,
        cache_validator: CacheValidator | None = None,
    ) -> LLMResult:
        """Full-history chat completion; a thin alias for :meth:`complete`.

        The caller owns the whole message list. Kept for backward compatibility
        with callers that still think in terms of a distinct chat method.
        """
        return await self.complete(
            task=task,
            model=model,
            prompt_version=prompt_version,
            messages=messages,
            response_format=response_format,
            cache_validator=cache_validator,
        )


def build_client(
    *,
    provider: str,
    engine: Engine,
    api_keys: dict[str, str] | None = None,
    timeout: float = 60.0,
    max_output_tokens: int | None = None,
    custom_base_url: str = "",
    custom_api_key_env: str = "",
) -> LLMClient:
    """Construct an :class:`LLMClient` from a provider name + credentials.

    ``api_keys`` maps env-var names (see ``PROVIDERS``) to values, keeping
    secrets out of config.toml. ``custom_*`` carry the custom provider's
    ``[llm] base_url`` / ``api_key_env`` overrides. Unknown provider names
    fail loudly with the valid options.
    """
    spec = PROVIDERS.get(provider)
    if spec is None:
        raise KeyError(f"unknown llm provider {provider!r}; valid: {', '.join(sorted(PROVIDERS))}")
    keys = api_keys or {}
    p: Provider
    if spec.kind == "openrouter":
        p = OpenRouterProvider(
            keys.get(spec.env_var, ""), timeout=timeout, max_tokens=max_output_tokens
        )
    else:
        p = _compat_provider(
            spec,
            keys,
            timeout=timeout,
            max_output_tokens=max_output_tokens,
            custom_base_url=custom_base_url,
            custom_api_key_env=custom_api_key_env,
        )
    return LLMClient(provider=p, engine=engine)


def _compat_provider(
    spec: ProviderSpec,
    keys: dict[str, str],
    *,
    timeout: float,
    max_output_tokens: int | None,
    custom_base_url: str,
    custom_api_key_env: str,
) -> OpenAICompatProvider:
    """An openai/anthropic/custom connection; custom overrides its spec."""
    is_custom = spec.name == "custom"
    base_url = custom_base_url if is_custom else ""
    env_var = custom_api_key_env if is_custom and custom_api_key_env else spec.env_var
    return OpenAICompatProvider(
        spec,
        keys.get(env_var, ""),
        timeout=timeout,
        max_tokens=max_output_tokens,
        base_url=base_url,
    )