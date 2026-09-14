"""Provider-agnostic LLM client: caching, cost + latency logging, retry policy.

The actual model call is delegated to a :class:`rigger.llm.providers.Provider`
(OpenRouter, LiteLLM SDK, or LiteLLM Proxy). This client owns the cache keyed on
``sha256(model + prompt_version + prompt)`` and persists every call to the
``llmcall`` table for cost tracking and backtest replay.
"""

from __future__ import annotations

import hashlib
import time
import uuid
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from rigger.core.db import LLMCallTable
from rigger.llm.providers import (
    LiteLLMProxyProvider,
    LiteLLMSDKProvider,
    OpenRouterProvider,
    Provider,
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
        raw = f"{model}\0{prompt_version}\0{prompt}".encode()
        return hashlib.sha256(raw).hexdigest()

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
    openrouter_api_key: str = "",
    litellm_proxy_key: str = "",
    proxy_base_url: str = "http://localhost:4000",
    timeout: float = 60.0,
) -> LLMClient:
    """Construct an :class:`LLMClient` from a provider name + credentials."""
    if provider == "openrouter":
        p: Provider = OpenRouterProvider(openrouter_api_key, timeout=timeout)
    elif provider == "litellm-proxy":
        p = LiteLLMProxyProvider(
            base_url=proxy_base_url,
            api_key=litellm_proxy_key,
            timeout=timeout,
        )
    else:
        p = LiteLLMSDKProvider(timeout=timeout)

    return LLMClient(provider=p, engine=engine)
