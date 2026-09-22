"""Tests for task -> model routing, including single-model and per-plugin overrides."""

from __future__ import annotations

import pytest

from delta.llm.router import model_for
from tests.conftest import FakeConfig


class Config:
    def __init__(self, model=None, routing=None, plugins=None):
        self.llm_model = model or ""
        self.llm_routing = routing or {}
        self.plugins = plugins or {}


def test_single_model_beats_everything():
    """One model set in Settings is the model for all tasks, plugin calls included."""
    cfg = Config(
        model="openai/gpt-4o",
        routing={"analyse": "anthropic/claude-sonnet-4"},
        plugins={"llm_analyst": {"model": "meta/llama-3.1-8b"}},
    )
    assert model_for(cfg, "analyse", plugin="llm_analyst") == "openai/gpt-4o"
    assert model_for(cfg, "anything", plugin="whatever") == "openai/gpt-4o"


def test_empty_single_model_is_unset():
    cfg = Config(model="", routing={"analyse": "openai/gpt-4o"})
    assert model_for(cfg, "analyse") == "openai/gpt-4o"


def test_plugin_override_beats_task_route():
    cfg = Config(
        routing={"analyse": "openai/gpt-4o"},
        plugins={"llm_analyst": {"model": "anthropic/claude-sonnet-4"}},
    )
    assert model_for(cfg, "analyse", plugin="llm_analyst") == "anthropic/claude-sonnet-4"


def test_task_route_used_when_no_override():
    cfg = Config(routing={"analyse": "openai/gpt-4o"}, plugins={"llm_analyst": {}})
    assert model_for(cfg, "analyse", plugin="llm_analyst") == "openai/gpt-4o"


def test_unknown_plugin_table_falls_back_to_task_route():
    cfg = Config(routing={"analyse": "openai/gpt-4o"})
    assert model_for(cfg, "analyse", plugin="llm_analyst") == "openai/gpt-4o"


def test_empty_string_override_ignored():
    cfg = Config(routing={"analyse": "openai/gpt-4o"}, plugins={"llm_analyst": {"model": ""}})
    assert model_for(cfg, "analyse", plugin="llm_analyst") == "openai/gpt-4o"


def test_keyerror_names_config_key_when_neither_exists():
    cfg = Config(routing={}, plugins={"llm_analyst": {"enabled": True}})
    with pytest.raises(KeyError) as exc:
        model_for(cfg, "analyse", plugin="llm_analyst")
    assert "set [llm] model" in str(exc.value)
    assert "add [llm.routing].analyse" in str(exc.value)


def test_existing_callers_without_plugin_kwarg_untouched():
    cfg = FakeConfig(llm_routing={"extract": "openai/gpt-4o"})
    assert model_for(cfg, "extract") == "openai/gpt-4o"
