"""Provider-agnostic LLM client: caching, cost + latency logging, retry policy.

The actual model call is delegated to a :class:`rigger.llm.providers.Provider`
(OpenRouter, or any OpenAI-compatible endpoint). This client owns the cache
keyed on ``sha256(model + prompt_version + prompt)`` and persists every call to
the ``llmcall`` table for cost tracking and backtest replay.
"""

from __future__ import annotations

import time
import uuid
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from rigger.core.db import LLMCallTable
from rigger.core.ids import stable_id
from rigger.llm.providers import (
    PROVIDERS,
    OpenAICompatProvider,
    OpenRouterProvider,
    Provider,
    ProviderSpec,
)

SYSTEM_PROMPT = "You are an investment analyst."


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
        prompt: str,
        response_format: dict[str, Any] | None = None,
    ) -> LLMResult:
        phash = self.prompt_hash(model, prompt_version, prompt)

        cached = self._lookup_cache(phash)
        if cached is not None:
            return LLMResult(text=cached, call_id="", cost_usd=0.0, cached=True)

        messages: list[dict[str, str]] = [
            {"role": "system", "content": SYSTEM_PROMPT},
            {"role": "user", "content": prompt},
        ]

        started = time.perf_counter()
        result = await self.provider.complete(
            model=model,
            messages=messages,
            response_format=response_format,
        )
        latency_ms = int((time.perf_counter() - started) * 1000)

        call_id = uuid.uuid4().hex
        self._store(
            LLMCallTable(
                id=call_id,
                ts=datetime.now(UTC),
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
    ) -> LLMResult:
        """Full-history chat completion: the caller owns the whole message list.

        Mirrors :meth:`complete` (cache keyed on the serialized transcript, one
        ``llmcall`` row per live call) but passes ``messages`` straight through
        to the provider instead of building a ``[system, user]`` pair.
        """
        prompt = "\n\n".join(f"{m['role']}: {m['content']}" for m in messages)
        phash = self.prompt_hash(model, prompt_version, prompt)

        cached = self._lookup_cache(phash)
        if cached is not None:
            return LLMResult(text=cached, call_id="", cost_usd=0.0, cached=True)

        started = time.perf_counter()
        result = await self.provider.complete(
            model=model,
            messages=messages,
            response_format=response_format,
        )
        latency_ms = int((time.perf_counter() - started) * 1000)

        call_id = uuid.uuid4().hex
        self._store(
            LLMCallTable(
                id=call_id,
                ts=datetime.now(UTC),
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
        )
        return LLMResult(
            text=result.text,
            call_id=call_id,
            cost_usd=result.cost_usd,
            cached=False,
        )

    def _lookup_cache(self, phash: str) -> str | None:
        with Session(self.engine) as session:
            row = session.exec(
                select(LLMCallTable).where(LLMCallTable.prompt_hash == phash)
            ).first()
            if row is not None and row.response is not None:
                return row.response
        return None

    def _store(self, row: LLMCallTable) -> None:
        with Session(self.engine) as session:
            session.add(row)
            session.commit()


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
