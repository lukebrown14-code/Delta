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

from rigger.llm.catalog import ModelInfo

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

    async def models(self, *, force: bool = False) -> list[ModelInfo]:
        """Models this provider offers: id, name, context length, USD-per-token prices.

        Implementations never raise: an empty catalog degrades to free-text
        model entry. ``force`` bypasses any 24h cache.
        """
        return []


class OpenRouterProvider(Provider):
    name = "openrouter"

    def __init__(
        self,
        api_key: str,
        *,
        app_name: str = "rigger",
        app_url: str = "https://github.com/lukebrown14-code/Rigger",
        timeout: float = 60.0,
        max_retries: int = 5,
    ) -> None:
        self.api_key = api_key
        self._app_name = app_name
        self._app_url = app_url
        self._timeout = timeout
        self._max_retries = max_retries
        self._pricing: dict[str, dict[str, float]] = {}
        self._models: list[ModelInfo] = []
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

    async def models(self, *, force: bool = False) -> list[ModelInfo]:
        self._maybe_load_pricing(force=force)
        return list(self._models)

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
            models: list[ModelInfo] = []
            for m in r.json().get("data", []):
                p = m.get("pricing", {})
                prompt_price = _parse_price(p.get("prompt"))
                completion_price = _parse_price(p.get("completion"))
                pricing[m["id"]] = {"prompt": prompt_price, "completion": completion_price}
                models.append(
                    ModelInfo(
                        id=m["id"],
                        name=m.get("name") or m["id"],
                        context_length=_parse_context_length(m.get("context_length")),
                        prompt_price=prompt_price,
                        completion_price=completion_price,
                    )
                )
            self._pricing = pricing
            self._models = models
            self._pricing_loaded_at = time.time()
        except Exception:
            self._pricing_loaded_at = time.time()


class LiteLLMSDKProvider(Provider):
    name = "litellm"

    def __init__(self, *, timeout: float = 60.0, num_retries: int = 5) -> None:
        self._timeout = timeout
        self._num_retries = num_retries

    async def models(self, *, force: bool = False) -> list[ModelInfo]:
        """Map litellm's bundled model_cost table locally; no network involved."""
        try:
            from litellm import model_cost

            return [
                ModelInfo(
                    id=str(model_id),
                    name=str(model_id),
                    context_length=_parse_context_length(
                        info.get("max_input_tokens") or info.get("max_tokens")
                    ),
                    prompt_price=_parse_price(info.get("input_cost_per_token")),
                    completion_price=_parse_price(info.get("output_cost_per_token")),
                )
                for model_id, info in model_cost.items()
                if isinstance(info, dict) and info.get("mode") == "chat"
            ]
        except Exception:
            return []

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

    async def models(self, *, force: bool = False) -> list[ModelInfo]:
        """GET {base_url}/v1/models; ids only, prices unknown (0.0), never raises."""
        try:
            import httpx

            headers = {"Authorization": f"Bearer {self.api_key}"} if self.api_key else {}
            async with httpx.AsyncClient(timeout=15.0) as client:
                r = await client.get(f"{self.base_url}/v1/models", headers=headers)
            r.raise_for_status()
            return [
                ModelInfo(
                    id=str(m["id"]),
                    name=str(m.get("name") or m["id"]),
                    context_length=_parse_context_length(m.get("context_length")),
                    prompt_price=0.0,
                    completion_price=0.0,
                )
                for m in r.json().get("data", [])
                if m.get("id")
            ]
        except Exception:
            return []


def _parse_context_length(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _parse_price(value: Any) -> float:
    if value is None:
        return 0.0
    try:
        return float(value)
    except (TypeError, ValueError):
        return 0.0
