"""Tests for the provider-agnostic LLM client."""

from __future__ import annotations

import asyncio

from rigger.llm.client import build_client
from rigger.llm.providers import (
    LiteLLMProxyProvider,
    LiteLLMSDKProvider,
    OpenRouterProvider,
    ProviderResult,
)


def test_build_client_selects_provider(tmp_engine):
    c = build_client(engine=tmp_engine, provider="litellm")
    assert isinstance(c.provider, LiteLLMSDKProvider)

    c = build_client(engine=tmp_engine, provider="openrouter", openrouter_api_key="k")
    assert isinstance(c.provider, OpenRouterProvider)

    c = build_client(
        engine=tmp_engine,
        provider="litellm-proxy",
        litellm_proxy_key="k",
        proxy_base_url="http://localhost:4000",
    )
    assert isinstance(c.provider, LiteLLMProxyProvider)


class _FakeProvider:
    def __init__(self) -> None:
        self.calls = 0

    async def complete(self, **kwargs):
        self.calls += 1
        return ProviderResult(text="hello", input_tokens=10, output_tokens=5, cost_usd=0.001)


def test_client_logs_and_caches(tmp_engine):
    from rigger.llm.client import LLMClient

    provider = _FakeProvider()
    client = LLMClient(provider=provider, engine=tmp_engine)

    r1 = asyncio.run(
        client.complete(
            task="analyse",
            model="m",
            prompt_version="v1",
            prompt="same prompt",
        )
    )
    assert r1.cached is False
    assert r1.text == "hello"
    assert provider.calls == 1

    r2 = asyncio.run(
        client.complete(
            task="analyse",
            model="m",
            prompt_version="v1",
            prompt="same prompt",
        )
    )
    assert r2.cached is True
    assert r2.cost_usd == 0.0
    assert provider.calls == 1  # cache hit, no second call
