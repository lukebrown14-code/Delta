"""LLM providers: OpenRouter plus any OpenAI-compatible endpoint.

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

#: Credit-balance cache lifetime; auto-fit reads ``/api/v1/credits`` at most this often.
CREDITS_TTL_SECONDS = 60.0

#: OpenRouter pre-reserves ``prompt + max_tokens * completion_price`` against the
#: credit balance. Below this cap a structured report cannot fit, so the provider
#: fails with guidance instead of sending a request that can never pass the check.
MAX_TOKENS_FLOOR = 512

#: Safety margin on the balance: the prompt-cost estimate is a chars/4 heuristic,
#: so auto-fit only commits 95% of the visible remainder to reserved output.
_BALANCE_MARGIN = 0.95


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


@dataclass(frozen=True)
class ProviderSpec:
    """Everything ``build_client`` needs to construct one provider connection.

    ``kind`` picks the implementation: ``openai-compat`` covers OpenAI,
    Anthropic's compat layer, and self-hosted servers; ``openrouter`` gets its
    own subclass. ``verify_path`` is the cheap authenticated GET used to prove
    a key works.
    """

    name: str
    base_url: str
    env_var: str
    kind: str = "openai-compat"
    verify_path: str = "/models"


PROVIDERS: dict[str, ProviderSpec] = {
    "openrouter": ProviderSpec(
        "openrouter", OPENROUTER_BASE_URL, "OPENROUTER_API_KEY", "openrouter", "/credits"
    ),
    "openai": ProviderSpec("openai", "https://api.openai.com/v1", "OPENAI_API_KEY"),
    "anthropic": ProviderSpec("anthropic", "https://api.anthropic.com/v1", "ANTHROPIC_API_KEY"),
    "custom": ProviderSpec("custom", "", "CUSTOM_API_KEY"),
}


class OpenAICompatProvider(Provider):
    """One connection to any OpenAI chat-completions compatible endpoint.

    OpenAI, Anthropic's compat layer, and self-hosted or aggregator servers
    (Ollama, Groq, Together...) all speak this protocol; the spec carries the
    per-vendor base URL and env var, the call path is identical.
    """

    def __init__(
        self,
        spec: ProviderSpec,
        api_key: str,
        *,
        timeout: float = 60.0,
        max_retries: int = 5,
        max_tokens: int | None = None,
        base_url: str = "",
    ) -> None:
        self.spec = spec
        self.name = spec.name
        self.api_key = api_key
        self.base_url = base_url or spec.base_url
        self._timeout = timeout
        self._max_retries = max_retries
        self._max_tokens = max_tokens
        self._client: Any = None

    def _extra_headers(self) -> dict[str, str]:
        return {}

    def _get_client(self) -> Any:
        from openai import AsyncOpenAI

        if self._client is None:
            if not self.base_url:
                raise RuntimeError(
                    f"{self.spec.name} provider requires a base URL; "
                    "set [llm] base_url in config.toml"
                )
            if not self.api_key:
                raise RuntimeError(f"{self.spec.env_var or 'api key'} is not set.")
            self._client = AsyncOpenAI(
                base_url=self.base_url,
                api_key=self.api_key,
                timeout=self._timeout,
                max_retries=self._max_retries,
                default_headers=self._extra_headers(),
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
        if self._max_tokens:
            kwargs["max_tokens"] = self._max_tokens
        if response_format:
            kwargs["response_format"] = response_format
        resp = await client.chat.completions.create(**kwargs)
        return self._result(resp, model)

    def _result(self, resp: Any, model: str) -> ProviderResult:
        """Normalize a chat-completion response into text/tokens/cost."""
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
        return 0.0  # no pricing table for generic compat providers

    async def models(self, *, force: bool = False) -> list[ModelInfo]:
        """GET {base_url}/models; ids only, prices unknown (0.0), never raises."""
        if not self.base_url:
            return []
        try:
            import httpx

            r = httpx.get(f"{self.base_url.rstrip('/')}/models", timeout=15.0)
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


class OpenRouterProvider(OpenAICompatProvider):
    """OpenRouter: one balance, every vendor's models.

    Adds on top of the compat call path: live per-token pricing, credit
    auto-fit (OpenRouter pre-reserves ``prompt + max_tokens * price`` against
    the balance), and app attribution headers.
    """

    def __init__(
        self,
        api_key: str,
        *,
        app_name: str = "rigger",
        app_url: str = "https://github.com/lukebrown14-code/Rigger",
        timeout: float = 60.0,
        max_retries: int = 5,
        max_tokens: int | None = None,
    ) -> None:
        super().__init__(
            PROVIDERS["openrouter"],
            api_key,
            timeout=timeout,
            max_retries=max_retries,
            max_tokens=max_tokens,
        )
        self._app_name = app_name
        self._app_url = app_url
        self._pricing: dict[str, dict[str, float]] = {}
        self._models: list[ModelInfo] = []
        self._pricing_loaded_at: float = 0.0
        self._credits: float | None = None
        self._credits_loaded_at: float = 0.0

    def _extra_headers(self) -> dict[str, str]:
        return {"HTTP-Referer": self._app_url, "X-Title": self._app_name}

    async def complete(
        self,
        *,
        model: str,
        messages: list[dict[str, str]],
        response_format: dict[str, Any] | None,
    ) -> ProviderResult:
        client = self._get_client()
        kwargs: dict[str, Any] = {"model": model, "messages": messages}
        cap = self._request_cap(model, messages)
        if cap is not None:
            kwargs["max_tokens"] = cap
        if response_format:
            kwargs["response_format"] = response_format
        try:
            resp = await client.chat.completions.create(**kwargs)
        except Exception as exc:
            if not _is_status_402(exc):
                raise
            previous = kwargs.get("max_tokens")
            retry_cap = self._request_cap(model, messages, force_credits=True)
            if retry_cap is None or (previous is not None and retry_cap >= previous):
                raise self._budget_error(model, messages) from exc
            kwargs["max_tokens"] = retry_cap
            resp = await client.chat.completions.create(**kwargs)
        return self._result(resp, model)

    def _request_cap(
        self,
        model: str,
        messages: list[dict[str, str]],
        *,
        force_credits: bool = False,
    ) -> int | None:
        """The ``max_tokens`` to send, or None when the provider default should apply.

        Fits the configured cap into the remaining OpenRouter credit balance
        using the cached pricing table, so the request passes OpenRouter's
        upfront ``prompt + reserved output <= credits`` check. Auto-fit is
        skipped (the configured cap is returned unchanged) when the balance or
        the model's pricing is unknown or the model is free.
        """
        credits = self._fetch_credits(force=force_credits)
        if credits is None:
            return self._max_tokens
        self._maybe_load_pricing()
        pricing = self._pricing.get(model)
        if pricing is None or pricing["completion"] <= 0.0:
            return self._max_tokens
        prompt_cost = _estimate_prompt_cost(messages, pricing["prompt"])
        budget = credits * _BALANCE_MARGIN - prompt_cost
        affordable = int(budget / pricing["completion"]) if budget > 0.0 else 0
        if affordable < MAX_TOKENS_FLOOR:
            raise self._budget_error(model, messages, credits=credits, pricing=pricing)
        if self._max_tokens is None:
            return affordable
        return min(self._max_tokens, affordable)

    def _fetch_credits(self, *, force: bool = False) -> float | None:
        """Remaining OpenRouter credits (``total_credits - total_usage``), cached.

        Never raises: on failure the last known balance is returned so a
        transient outage degrades to the configured cap instead of an error.
        """
        if (
            not force
            and self._credits is not None
            and (time.time() - self._credits_loaded_at) < CREDITS_TTL_SECONDS
        ):
            return self._credits
        if not self.api_key:
            return None
        try:
            import httpx

            r = httpx.get(
                f"{OPENROUTER_BASE_URL}/credits",
                headers={"Authorization": f"Bearer {self.api_key}"},
                timeout=10.0,
            )
            r.raise_for_status()
            data = r.json().get("data", {})
            total = float(data.get("total_credits") or 0.0)
            usage = float(data.get("total_usage") or 0.0)
            self._credits = total - usage
            self._credits_loaded_at = time.time()
        except Exception:
            pass
        return self._credits

    def _budget_error(
        self,
        model: str,
        messages: list[dict[str, str]],
        *,
        credits: float | None = None,
        pricing: dict[str, float] | None = None,
    ) -> RuntimeError:
        """Actionable 402-style error: numbers first, remedies second."""
        credits = self._credits if credits is None else credits
        pricing = self._pricing.get(model) if pricing is None else pricing
        detail = ""
        if credits is not None and pricing and pricing["completion"] > 0.0:
            need = _estimate_prompt_cost(messages, pricing["prompt"]) + (
                (self._max_tokens or MAX_TOKENS_FLOOR) * pricing["completion"]
            )
            detail = f" (needs up to ~${need:.2f} for {model})"
        balance = f"${credits:.2f} remaining" if credits is not None else "unknown balance"
        return RuntimeError(
            f"OpenRouter credits ({balance}) cannot cover this request{detail}. "
            "Add credits at https://openrouter.ai/settings/credits, lower "
            "[llm] max_output_tokens in config.toml, or route a cheaper model "
            "in [llm.routing]."
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


async def verify_key(spec: ProviderSpec, api_key: str, *, base_url: str = "") -> bool:
    """One authenticated GET proving a key works; never raises.

    Custom providers pass their configured ``base_url``.
    """
    if not api_key:
        return False
    base = (base_url or spec.base_url).rstrip("/")
    if not base:
        return False
    headers = {"Authorization": f"Bearer {api_key}"}
    if spec.name == "anthropic":
        # Anthropic's native API authenticates with x-api-key, not Bearer.
        headers["x-api-key"] = api_key
        headers["anthropic-version"] = "2023-06-01"
    try:
        import httpx

        async with httpx.AsyncClient(timeout=10.0) as client:
            r = await client.get(f"{base}{spec.verify_path}", headers=headers)
        return r.status_code == 200
    except Exception:
        return False


def _is_status_402(exc: BaseException) -> bool:
    """True when ``exc`` is OpenRouter's credit-budget rejection (402)."""
    from openai import APIStatusError

    return isinstance(exc, APIStatusError) and exc.status_code == 402


def _estimate_prompt_cost(messages: list[dict[str, str]], prompt_price: float) -> float:
    """Rough prompt cost: chars/4 as the token count, times the per-token price."""
    chars = sum(len(m.get("content") or "") for m in messages)
    return (chars / 4.0) * prompt_price
