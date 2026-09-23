"""Data-source setup contracts and credential persistence."""

from __future__ import annotations

from types import SimpleNamespace

from delta.core.config import CONFIG_PATH, load_toml, read_env_value, update_config
from delta.core.plugin import DataPlugin, DataProviderField, DataProviderSpec
from delta.services import configure_data_provider, remove_market, save_market


class LicensedSource(DataPlugin):
    name = "licensed"
    provider_spec = DataProviderSpec(
        label="Licensed source",
        fields=(
            DataProviderField("endpoint", "Endpoint", required=True),
            DataProviderField("api_key", "API key", required=True, secret=True, env_var="FT_API_KEY"),
        ),
    )


class FakeRig:
    def __init__(self) -> None:
        self.plugins = {"licensed": LicensedSource()}
        self.cfg = SimpleNamespace(plugins={})
        self.reloaded = 0

    def reload_data_sources(self) -> None:
        self.reloaded += 1


def test_licensed_source_persists_key_only_in_dotenv(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    delta = FakeRig()
    save_market("lse", label="London", currency="GBP", yahoo_suffix=".L")
    configure_data_provider(
        delta,
        "licensed",
        {"endpoint": "https://api.example.test/v1", "api_key": "secret-value"},
        markets=["lse"],
    )
    text = (tmp_path / "config.toml").read_text(encoding="utf-8")
    assert "https://api.example.test/v1" in text
    assert "secret-value" not in text
    assert 'markets = [\n    "lse",\n]' in text
    assert read_env_value("FT_API_KEY") == "secret-value"
    assert delta.reloaded == 1
    try:
        remove_market("lse")
    except ValueError as exc:
        assert "licensed" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("referenced market was removed")


def test_licensed_source_rejects_missing_required_key(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    try:
        configure_data_provider(
            FakeRig(), "licensed", {"endpoint": "https://api.example.test", "api_key": ""}
        )
    except ValueError as exc:
        assert "API key is required" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("missing required key was accepted")


def test_update_config_mutates_and_respects_custom_path(monkeypatch, tmp_path):
    """C6/A6: one helper loads, mutates and writes; a real ``CONFIG_PATH`` is honoured."""
    custom = tmp_path / "elsewhere.toml"
    custom.write_text("[llm]\nprovider = \"openrouter\"\n", encoding="utf-8")

    update_config(lambda raw: raw.setdefault("llm", {}).update(provider="custom"), path=custom)

    assert load_toml(custom)["llm"]["provider"] == "custom"


def test_update_config_writes_to_configured_path_not_cwd_file(monkeypatch, tmp_path):
    """A write land against the default path in the current working directory."""
    monkeypatch.chdir(tmp_path)
    update_config(lambda raw: raw.setdefault("targets", {}).setdefault("t", {}).update(kind="company"))

    assert CONFIG_PATH.exists()
    assert "company" in CONFIG_PATH.read_text(encoding="utf-8")


def test_runtime_reload_parts_rebuild_only_what_is_asked(monkeypatch, tmp_path):
    """C7: one ``reload(parts)``; ``llm`` leaves plugins/targets alone, ``targets`` rebuilds them."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[llm]\nprovider = "openrouter"\n[targets.apple]\nkind = "company"\nmarket = "us"\ntickers = ["AAPL"]\n',
        encoding="utf-8",
    )
    from delta.runtime import Delta

    delta = Delta()
    targets_before = delta.targets
    llm_before = delta.llm

    delta.reload("llm")
    assert delta.llm is not llm_before
    assert delta.targets is targets_before

    delta.reload("targets")
    assert delta.targets is not targets_before
