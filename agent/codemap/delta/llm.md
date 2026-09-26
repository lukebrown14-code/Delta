# delta/llm

All LLM access lives here. Depends on `delta/core` (db for call cache, config). Depended on by `extract`, `reports`, `chat`, `theses`, `thesis_summary`, `sentiment`.

## Files

- `client.py` — LLM facade: `LLMClient`, `LLMResult`, `build_client` (from config + provider), `SYSTEM_PROMPT`
- `providers.py` — provider specs + calls: `PROVIDERS` dict, `Provider` ABC, `OpenAICompatProvider`, `OpenRouterProvider`, `verify_key`, `ProviderSpec`
- `router.py` — task→model resolution: `model_for(config, task, plugin=...)`
- `structured.py` — typed LLM calls: `structured()` generic (pydantic T), `render_prompt` (jinja2 from `prompts/`), `schema_from_model`
- `cache.py` — response cache in db: `lookup_cache`, `store_call` (LLMCallTable)
- `catalog.py` — model listing for pickers: `catalog`, `cached_catalog`, `ModelInfo`, plus config writers `set_llm_route`/`set_llm_model`/`set_llm_provider`/`set_llm_custom`/`set_plugin_model`; cache file `data/model_catalog.json`
- `json.py` — `extract_json` (parse JSON out of model text)
- `jev.py` — Jev judgment client: `JevClient`, `ChoiceQuestion`, `Usage`, `JevError`, `JEV_MODEL`
- `eval.py` — citation eval: `valid_citations`, `hallucinated_citations`, `citation_validity_rate`, `GoldenCase`, `evaluate`
- `prompts/` — versioned jinja2 templates: `extract_v1.j2`, `report_v1.j2`, `report_v2.j2`, `thesis_v1.j2`, `thesis_summary_v1.j2` (chat prompt is inline in `delta/chat.py`)

## To change Y, edit Z

- Add an LLM task → new `prompts/<task>_v<N>.j2` + caller module + route key in `config.toml` `[llm.routing]`
- Change a prompt → bump the version (new `_v<N>.j2`), keep grounded/cited rules intact
- Add a provider → `providers.py` (`ProviderSpec` in `PROVIDERS`), key env name in `.env.example`
- Change caching → `cache.py`; poisoned/invalid JSON must not be cached

## Gotchas

- mypy strict is enforced on this package in CI.
- Prompt files are versioned artifacts — edit content in a new version rather than rewriting history.
- `jev.py` targets OpenRouter's decisions API (`DECISIONS_URL`), not the chat endpoint.
