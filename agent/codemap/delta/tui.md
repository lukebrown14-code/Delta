# delta/tui

Textual frontend. Depends on `delta/services.py`, `delta/runtime.py` (Delta), domain modules. Nothing depends on it except the `delta` script entry point.

## Files

- `app.py` — `DeltaApp(App)`, `run_tui` (script entry), `DeltaCommands` (command palette), `GoPicker`; keybindings 1–6/c/h/m/p/g/?/q/F2; `screens_by_name` registry; `_screens` dict in `on_mount` is where screens register
- `shell.py` — screen base classes: `DeltaScreen(Screen)`, `StatusBar`, `ScreenFooter`, `NavKey`, `age_text`
- `components.py` — shared mixins/helpers: `QuoteFeedMixin`, `SuggestionList`, `EmptyState`, `SectionHeading`, `goto`, `require_selection`, `quote_suffixes`, `thesis_from_citations`
- `widgets.py` — pane/chart widgets: `Pane`, `PaneRow`, `PaneStack`, `ActionChip`, `BrailleGraph`, `PriceChart`, `hint_markup`, `health_variant`, `sentiment_variant`
- `axes.py` — chart math/formatting: `nice_ticks`, `plot_scale`, `format_price`, `x_ticks`
- `theme.py` — `DELTA_DARK` (and light) `Theme` objects; text uses `text-*` tokens only
- `delta.tcss` — stylesheet
- `screens/home.py` — `Home`: dashboard, agenda, first-run checklist (`services.setup_checks`)
- `screens/targets.py` — `Targets`: watchlist, metric grid, `TargetAddModal`, `MetricHelpModal`
- `screens/research.py` — `Research` (base), `screens/data.py` `Data(Research)`, `screens/reports.py` `Reports(Research)`, `screens/research_viewer.py` `ResearchViewer(MarkdownViewer)`, `screens/research_state.py` `ResearchState`/`CompanyView` (shared view state)
- `screens/theses.py` — `Theses` fleet list
- `screens/thesis_form.py` — `ThesisForm` dialog; `screens/thesis_panes.py` — `HealthPane` detail panes
- `screens/chat.py` — `Chat`: grounded Q&A, `CiteRow` citations
- `screens/decisions.py` — `Decisions`, `DecisionForm`, `ReviewForm`
- `screens/config.py` — `Config`: provider/model/plugin/diagnostics settings
- `screens/provider_picker.py` — `ProviderPicker`, `KeyEntryModal`, `CustomFormModal`, `connect_provider` (press `p`)
- `screens/model_picker.py` — `ModelPicker` (press `m`); `screens/market_setup.py` — `MarketSetupModal`; `screens/source_setup.py` — `SourceSetupModal`, `configure_source`
- `screens/help.py` — `HelpScreen` keymap

## To change Y, edit Z

- Add a screen → new `screens/<name>.py` (inherit `DeltaScreen`) + entry in `app.py` `_screens` + binding
- Change layout → `delta.tcss` + `Pane`/`PaneRow`/`PaneStack` in `widgets.py`
- Change colors → `theme.py` (only `text-*` tokens in readable text rules; a test enforces this)
- Re-baseline snapshots after intentional UI change → `uv run pytest tests/test_snapshots.py --snapshot-update`

## Gotchas

- Screens must call `delta/services.py`, never storage or providers directly.
- Async/network work runs via workers — never block Textual event handlers.
- `tests/__snapshots__/` breaks on styling changes; re-baseline deliberately.
