"""LLM providers: OpenRouter, LiteLLM SDK, and LiteLLM Proxy.

Each provider knows how to make a raw chat completion call and how to turn the
call into text, token counts, and a USD cost estimate. The generic caching,
logging, and retry policy live in :mod:`rigger.llm.client`.
"""

from __future__ import annotations

import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from typing import Any

OPENROUTER_BASE_URL = "https://openrouter.ai/api/v1"


@dataclass
class ProviderResult:
    text: str
    input_tokens: int
    output_tokens: int
    cost_usd: float


class Provider(ABC):
    name: str

    @abstractmethod
    async def complete(
        self,
        *,
        model: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None,
    ) -> ProviderResult:
        """Perform a chat completion and return normalized text/tokens/cost."""


class OpenRouterProvider(Provider):
    name = "openrouter"

    def __init__(
        self,
        api_key: str,
        *,
        app_name: str = "rigger",
        app_url: str = "https://github.com/luke/rigger",
        timeout: float = 60.0,
        max_retries: int = 5,
    ) -> None:
        self.api_key = api_key
        self._app_name = app_name
        self._app_url = app_url
        self._timeout = timeout
        self._max_retries = max_retries
        self._pricing: dict[str, dict[str, float]] = {}
        self._pricing_loaded_at: float = 0.0
        self._client: Any = None

    def _get_client(self) -> Any:
        from openai import AsyncOpenAI

        if self._client is None:
            if not self.api_key:
                raise RuntimeError("OPENROUTER_API_KEY is not set.")
            self._client = AsyncOpenAI(
                base_url=OPENROUTER_BASE_URL,
                api_key=self.api_key,
                timeout=self._timeout,
                max_retries=self._max_retries,
                default_headers={
                    "HTTP-Referer": self._app_url,
                    "X-Title": self._app_name,
                },
            )
        return self._client

    async def complete(
        self,
        *,
        model: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None,
    ) -> ProviderResult:
        client = self._get_client()
        kwargs: dict[str, Any] = {"model": model, "messages": messages}
        if response_format:
            kwargs["response_format"] = response_format
        resp = await client.chat.completions.create(**kwargs)
        text = resp.choices[0].message.content or ""
        usage = resp.usage
        input_tokens = usage.prompt_tokens if usage else 0
        output_tokens = usage.completion_tokens if usage else 0
        return ProviderResult(
            text=text,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            cost_usd=self._compute_cost(model, input_tokens, output_tokens),
        )

    def _compute_cost(self, model: str, input_tokens: int, output_tokens: int) -> float:
        self._maybe_load_pricing()
        pricing = self._pricing.get(model)
        if pricing is None:
            return 0.0
        return input_tokens * pricing["prompt"] + output_tokens * pricing["completion"]

    def _maybe_load_pricing(self, force: bool = False) -> None:
        if (
            not force
            and self._pricing_loaded_at
            and (time.time() - self._pricing_loaded_at) < 86400
        ):
            return
        if not self.api_key:
            return
        try:
            import httpx

            r = httpx.get(
                f"{OPENROUTER_BASE_URL}/models",
                headers={"Authorization": f"Bearer {self.api_key}"},
                timeout=15.0,
            )
            r.raise_for_status()
            pricing: dict[str, dict[str, float]] = {}
            for m in r.json().get("data", []):
                p = m.get("pricing", {})
                pricing[m["id"]] = {
                    "prompt": _parse_price(p.get("prompt")),
                    "completion": _parse_price(p.get("completion")),
                }
            self._pricing = pricing
            self._pricing_loaded_at = time.time()
        except Exception:
            self._pricing_loaded_at = time.time()


class LiteLLMSDKProvider(Provider):
    name = "litellm"

    def __init__(self, *, timeout: float = 60.0, num_retries: int = 5) -> None:
        self._timeout = timeout
        self._num_retries = num_retries

    async def complete(
        self,
        *,
        model: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None,
    ) -> ProviderResult:
        from litellm import acompletion, completion_cost

        kwargs: dict[str, Any] = {
            "model": model,
            "messages": messages,
            "timeout": self._timeout,
            "num_retries": self._num_retries,
        }
        if response_format:
            kwargs["response_format"] = response_format
        resp = await acompletion(**kwargs)
        text = resp.choices[0].message.content or ""
        usage = resp.usage
        input_tokens = usage.prompt_tokens if usage else 0
        output_tokens = usage.completion_tokens if usage else 0
        try:
            cost_usd = completion_cost(completion_response=resp)
        except Exception:
            cost_usd = 0.0
        return ProviderResult(
            text=text,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            cost_usd=float(cost_usd or 0.0),
        )


class LiteLLMProxyProvider(Provider):
    name = "litellm-proxy"

    def __init__(
        self,
        *,
        base_url: str = "http://localhost:4000",
        api_key: str = "",
        timeout: float = 60.0,
        max_retries: int = 5,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self._timeout = timeout
        self._max_retries = max_retries
        self._client: Any = None

    def _get_client(self) -> Any:
        from openai import AsyncOpenAI

        if self._client is None:
            self._client = AsyncOpenAI(
                base_url=f"{self.base_url}/v1",
                api_key=self.api_key or "no-key",
                timeout=self._timeout,
                max_retries=self._max_retries,
            )
        return self._client

    async def complete(
        self,
        *,
        model: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None,
    ) -> ProviderResult:
        client = self._get_client()
        kwargs: dict[str, Any] = {"model": model, "messages": messages}
        if response_format:
            kwargs["response_format"] = response_format
        resp = await client.chat.completions.create(**kwargs)
        text = resp.choices[0].message.content or ""
        usage = resp.usage
        input_tokens = usage.prompt_tokens if usage else 0
        output_tokens = usage.completion_tokens if usage else 0
        return ProviderResult(
            text=text,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            cost_usd=self._compute_cost(model, input_tokens, output_tokens),
        )

    @staticmethod
    def _compute_cost(model: str, input_tokens: int, output_tokens: int) -> float:
        try:
            from litellm import cost_per_token

            prompt_cost, completion_cost = cost_per_token(
                model, prompt_tokens=input_tokens, completion_tokens=output_tokens
            )
            return float(prompt_cost + completion_cost)
        except Exception:
            return 0.0


def _parse_price(value: Any) -> float:
    if value is None:
        return 0.0
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0
