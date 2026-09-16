"""Tests for the provider-agnostic LLM client."""

from __future__ import annotations

import asyncio

import pytest

from rigger.llm.client import build_client
from rigger.llm.providers import (
    OpenAICompatProvider,
    OpenRouterProvider,
    ProviderResult,
)

KEYS = {
    "OPENROUTER_API_KEY": "k",
    "OPENAI_API_KEY": "k",
    "ANTHROPIC_API_KEY": "k",
    "CUSTOM_API_KEY": "k",
}


def test_build_client_selects_provider(tmp_engine):
    c = build_client(engine=tmp_engine, provider="openrouter", api_keys=KEYS)
    assert isinstance(c.provider, OpenRouterProvider)


def test_build_client_resolves_compat_providers(tmp_engine):
    c = build_client(engine=tmp_engine, provider="openai", api_keys=KEYS)
    assert isinstance(c.provider, OpenAICompatProvider)
    assert c.provider.base_url == "https://api.openai.com/v1"
    assert c.provider.api_key == "k"

    c = build_client(engine=tmp_engine, provider="anthropic", api_keys=KEYS)
    assert isinstance(c.provider, OpenAICompatProvider)
    assert c.provider.base_url == "https://api.anthropic.com/v1"


def test_build_client_custom_uses_configured_url_and_env(tmp_engine):
    keys = {**KEYS, "MY_KEY": "mine"}
    c = build_client(
        engine=tmp_engine,
        provider="custom",
        api_keys=keys,
        custom_base_url="http://localhost:11434/v1",
        custom_api_key_env="MY_KEY",
    )
    assert isinstance(c.provider, OpenAICompatProvider)
    assert c.provider.base_url == "http://localhost:11434/v1"
    assert c.provider.api_key == "mine"


def test_build_client_unknown_provider_fails_loudly(tmp_engine):
    with pytest.raises(KeyError, match="valid:"):
        build_client(engine=tmp_engine, provider="nope")


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
