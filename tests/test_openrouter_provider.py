"""Offline tests for OpenRouter credit auto-fit and 402 handling (HTTP via respx)."""

from __future__ import annotations

import asyncio
import time
from types import SimpleNamespace
from typing import Any

import httpx
import respx
from openai import APIStatusError

from rigger.llm.providers import OPENROUTER_BASE_URL, OpenRouterProvider

CREDITS_URL = f"{OPENROUTER_BASE_URL}/credits"

PRICING = {"prompt": 3e-06, "completion": 1.5e-05}
MESSAGES = [{"role": "user", "content": "x" * 4000}]

#: $0.05 balance: 0.05*0.95 - (1000 tokens * 3e-6) = 0.0445 -> 2966 completion tokens.
LOW_BALANCE_CAP = 2966


def _provider(max_tokens: int | None = 4096) -> OpenRouterProvider:
    provider = OpenRouterProvider(api_key="k", max_tokens=max_tokens)
    provider._pricing = {"test/model": dict(PRICING)}
    provider._pricing_loaded_at = time.time()
    return provider


def _credits_response(balance_remaining: float) -> httpx.Response:
    return httpx.Response(
        200,
        json={"data": {"total_credits": balance_remaining, "total_usage": 0.0}},
    )


def _chat_response() -> Any:
    return SimpleNamespace(
        choices=[SimpleNamespace(message=SimpleNamespace(content="ok"))],
        usage=SimpleNamespace(prompt_tokens=10, completion_tokens=5),
    )


def _status_402() -> APIStatusError:
    request = httpx.Request("POST", f"{OPENROUTER_BASE_URL}/chat/completions")
    return APIStatusError("credits", response=httpx.Response(402, request=request), body=None)


class _FakeCompletions:
    """Records every create() call and replays outcomes (responses or exceptions)."""

    def __init__(self, outcomes: list[Any]) -> None:
        self._outcomes = list(outcomes)
        self.calls: list[dict[str, Any]] = []

    async def create(self, **kwargs: Any) -> Any:
        self.calls.append(kwargs)
        outcome = self._outcomes.pop(0)
        if isinstance(outcome, BaseException):
            raise outcome
        return outcome


def _fake_client(outcomes: list[Any]) -> _FakeCompletions:
    return _FakeCompletions(outcomes)


def _run_complete(
    provider: OpenRouterProvider,
    completions: _FakeCompletions,
    *,
    model: str = "test/model",
) -> Any:
    provider._client = SimpleNamespace(chat=SimpleNamespace(completions=completions))
    return asyncio.run(
        provider.complete(
            model=model,
            messages=MESSAGES,
            response_format=None,
        )
    )


@respx.mock
def test_low_balance_clamps_max_tokens(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=_credits_response(0.05))
    completions = _fake_client([_chat_response()])
    provider = _provider()

    _run_complete(provider, completions)

    assert completions.calls[0]["max_tokens"] == LOW_BALANCE_CAP


@respx.mock
def test_no_configured_cap_sends_affordable(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=_credits_response(0.05))
    completions = _fake_client([_chat_response()])
    provider = _provider(max_tokens=None)

    _run_complete(provider, completions)

    assert completions.calls[0]["max_tokens"] == LOW_BALANCE_CAP


@respx.mock
def test_unknown_or_free_model_keeps_configured_cap(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=_credits_response(0.05))
    free = _fake_client([_chat_response()])
    unknown = _fake_client([_chat_response()])

    provider = _provider()
    provider._pricing["free/model"] = {"prompt": 0.0, "completion": 0.0}
    _run_complete(provider, free, model="free/model")
    _run_complete(provider, unknown, model="other/model")

    assert free.calls[0]["max_tokens"] == 4096
    assert unknown.calls[0]["max_tokens"] == 4096


@respx.mock
def test_credits_fetch_failure_degrades_to_configured_cap(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=httpx.Response(500))
    completions = _fake_client([_chat_response()])
    provider = _provider()

    _run_complete(provider, completions)

    assert completions.calls[0]["max_tokens"] == 4096


@respx.mock
def test_below_floor_raises_friendly_error_without_request(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=_credits_response(0.001))
    completions = _fake_client([_chat_response()])
    provider = _provider()

    try:
        _run_complete(provider, completions)
        raise AssertionError("expected RuntimeError")
    except RuntimeError as exc:
        assert "openrouter.ai/settings/credits" in str(exc)
        assert "test/model" in str(exc)
    assert completions.calls == []


@respx.mock
def test_402_refreshes_balance_and_retries_once_lower(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    route = respx.get(CREDITS_URL).mock(
        side_effect=[_credits_response(5.0), _credits_response(0.05)]
    )
    completions = _fake_client([_status_402(), _chat_response()])
    provider = _provider()

    result = _run_complete(provider, completions)

    assert route.call_count == 2
    assert len(completions.calls) == 2
    assert completions.calls[0]["max_tokens"] == 4096
    assert completions.calls[1]["max_tokens"] == LOW_BALANCE_CAP
    assert result.text == "ok"


@respx.mock
def test_402_without_improvement_raises_friendly_error(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(CREDITS_URL).mock(return_value=_credits_response(5.0))
    completions = _fake_client([_status_402(), _status_402()])
    provider = _provider()

    try:
        _run_complete(provider, completions)
        raise AssertionError("expected RuntimeError")
    except RuntimeError as exc:
        assert "openrouter.ai/settings/credits" in str(exc)
    assert len(completions.calls) == 1
