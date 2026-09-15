# Model Selection Plan — opencode-style model choice for Rigger

> Make "which model runs this step" a thing you browse and pick, not a string
> you remember and hand-edit. Same working rules as Phase 2: no behaviour
> change without a test, `pytest` stays offline.

## Goal

From the TUI Config screen, see every LLM task and every LLM-using plugin with
the model it will use, open a searchable list of models the configured provider
actually offers (with context length and price), pick one, and have
`config.toml` updated. A plugin can override a task route, so `llm_analyst` and
a second analyst strategy can run different models on the same `analyse` task.

## Where we are today

- `[llm.routing]` in `config.toml` maps task -> model id: `extract`, `analyse`,
  `critique`, `pm`. Loaded into `AppConfig.llm_routing`
  (`rigger/core/config.py:37,73`).
- `model_for(config, task)` (`rigger/llm/router.py`) resolves a route and raises
  `KeyError` when unrouted — deliberately, so a missing route cannot silently
  fall back to a hard-coded literal. Call sites: `rigger/extract.py:69`,
  `rigger/plugins/strategies/llm_analyst.py:81`,
  `rigger/plugins/strategies/critic.py:58`.
- `ensemble` ignores routing entirely and reads `AppConfig.llm_ensemble_models`
  (`rigger/plugins/strategies/ensemble.py:33`).
- `OpenRouterProvider._maybe_load_pricing()` (`rigger/llm/providers.py`) already
  fetches the whole `GET /models` catalog every 24h and keeps only the prompt
  and completion prices.
- `set_plugin_enabled()` (`rigger/services.py:350`) is the one existing
  config-writeback path: `load_toml()` -> mutate -> `tomli_w.dumps` ->
  `config.toml`.

Three gaps: the catalog is thrown away, routing is task-global so two plugins
on one task cannot differ, and nothing writes routes back.

## Step 1 — Model catalog

New `rigger/llm/catalog.py`.

```python
@dataclass(frozen=True)
class ModelInfo:
    id: str                    # "anthropic/claude-sonnet-4"
    name: str                  # "Claude Sonnet 4"
    context_length: int | None
    prompt_price: float        # USD per token
    completion_price: float
```

1. Add `async def models(self) -> list[ModelInfo]` to the `Provider` ABC
   (`rigger/llm/providers.py`), default `return []`.
2. `OpenRouterProvider`: widen the existing `/models` parse to build
   `ModelInfo` objects, keep them alongside `_pricing` under the same 24h
   timestamp. `_compute_cost` keeps reading the same numbers — one fetch, two
   consumers, no second network call.
3. `LiteLLMProxyProvider`: `GET {base_url}/v1/models`, which returns ids only;
   fill price from `litellm.cost_per_token` where available, else `0.0`.
4. `LiteLLMSDKProvider`: `litellm.model_cost` is a local dict — map it straight
   to `ModelInfo`, no network.
5. Catalog failures never raise. An empty catalog means the picker degrades to
   free-text entry, exactly as today.

Cache the JSON under `data/model_catalog.json` with a fetched-at stamp so the
TUI opens instantly and `pytest` never needs the network.

## Step 2 — Per-plugin overrides

Extend the resolver, keep the loud failure:

```python
def model_for(config, task: str, plugin: str | None = None) -> str:
    """[plugins.<plugin>].model  ->  [llm.routing].<task>  ->  KeyError."""
```

- `config.plugins[plugin]["model"]` wins when present and non-empty.
- Otherwise `config.llm_routing[task]`, unchanged.
- Still `KeyError` with the same "add `[llm.routing].<task>` to config.toml"
  message when neither exists.

Update the three call sites to pass `plugin=self.name` (and `plugin="extract"`
is *not* used — `rigger/extract.py` is not a plugin, so it keeps the
task-only call).

`ensemble` keeps its own `models` list, but read it from its plugin table first
so it lands in the same place as everything else:
`[plugins.ensemble] models = [...]`, falling back to `[llm.ensemble] models`
for compatibility with the current `config.toml`.

## Step 3 — Config writeback

In `rigger/services.py`, beside `set_plugin_enabled`:

```python
def set_llm_route(rig, task: str, model: str) -> None      # [llm.routing].<task>
def set_plugin_model(rig, name: str, model: str | None) -> None  # [plugins.<n>].model, None removes
```

Both follow the existing `load_toml` -> mutate -> `tomli_w.dumps` shape. Two
notes that cost us once already:

- The plugin table key is the plugin's `name` attribute, not its entry-point
  name (`[plugins.yfinance]` vs entry point `yfinance_data`). Write by
  `plugin.name`.
- `tomli_w.dumps` of the loaded dict drops comments. `config.toml` currently
  carries explanatory comments; accept the loss on write, and note it in the
  Config screen footer so it is not a surprise.

Then a read-side query for the screen:

```python
@dataclass
class RouteRow:
    task: str
    model: str
    source: Literal["routing", "plugin", "unrouted"]
    plugin: str | None
    cost_usd_30d: float

def llm_routes(rig) -> list[RouteRow]
```

`cost_usd_30d` comes from the existing `llmcall` table grouped by task and
model — the same numbers screen `5` shows, so a model swap can be judged
against what it has been costing.

## Step 4 — The picker

Config screen (tab `6` in the TUI) gains a routing table: task,
model, source, 30-day spend. `enter` on a row opens a `ModelPicker` modal:

- `Input` filter over `ModelInfo.id` and `.name`, substring match,
  case-insensitive.
- `DataTable` of id, context length, $/1M prompt, $/1M completion.
- `enter` selects, `escape` cancels, `r` forces a catalog refetch.
- Selecting on a task row calls `set_llm_route`; on a plugin row,
  `set_plugin_model`. `d` on a plugin row clears the override back to the task
  route.
- After a write, reload `rig.cfg` so the change is live without restarting.

CLI parity, since the CLI stays the scripting surface:

```
rig models                      # list the catalog, optional --filter
rig route analyse <model-id>    # set a task route
rig route --plugin critic <id>  # set a plugin override, --clear to remove
```

## Tests

Offline, as always.

- `tests/test_catalog.py` — `respx` mock of OpenRouter `/models`, assert
  `ModelInfo` parsing, the 24h cache, and that a failed fetch yields `[]` and
  does not raise.
- `tests/test_router.py` — plugin override beats task route; task route used
  when no override; empty-string override ignored; `KeyError` when neither.
- `tests/test_services.py` — `set_llm_route` and `set_plugin_model` round-trip
  through a `tmp_path` `config.toml`; `set_plugin_model(None)` removes the key.
- `tests/test_tui_config.py` — `App.run_test()`: open the picker, filter,
  select, assert the written TOML and the reloaded config.

The existing ensemble and critic tests must stay green untouched — the new
`plugin=` argument is keyword-only with a default.

## Order of work

1. Step 2 (`model_for` override) — small, self-contained, no UI.
2. Step 1 (catalog) — the only part that touches the network.
3. Step 3 (writeback + `llm_routes`) — CLI usable at this point.
4. Step 4 (picker) — lands with the rest of the Config screen.

## Out of scope

Per-instrument or per-run model choice, automatic fallback chains when a model
errors, and any spend cap that refuses a call. Routing stays declarative.
