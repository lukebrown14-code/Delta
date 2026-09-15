# Chat Plan

> Grounded Q&A over the evidence pool, with opt-in web search as an explicit tool.

## Goal

A user asks "what changed in solar demand this quarter?" and gets an answer that
is (a) grounded in stored evidence, (b) optionally informed by a live web search
the user explicitly allowed, and (c) fully cited with each fact labelled
`Stored` / `Web` / `AI inference`.

## Design

- Local-first: the assistant's default context is the target(s) the user selects
  plus the evidence pool, assembled the same way reports assemble their context.
- Web search is a **tool the user turns on**, not a default. When on, the model
  may issue one-or-more searches via a bound tool; results are returned to the
  model as `Web` evidence and are never written to the stored pool.
- Every answer ends with citations. A fact without a citation is downgraded to
  "I can't support this from your data."
- Conversation history is in-memory per session; optionally persisted later.

## Model

```python
class ChatMessage:
    role: Literal["user", "assistant", "tool"]
    text: str
    citations: tuple[str, ...]
    source: Literal["stored", "web", "inference", "user"]
```

## Components

- `services.chat(rig, history, targets)` -> `ChatMessage` via the LLM client's
  chat path (a non-structured path alongside `structured()`).
- A `SearchTool` behind a provider-agnostic interface, stubbed offline; a real
  implementation is a later, separately-gated plugin.
- TUI Chat screen: context selector, allow-web toggle, transcript, input. Model
  picker applies (see Model Selection plan).

## Tests

`tests/test_chat.py`: `FakeLLM` echoes with a canned citation; web toggle gates
the search tool; search results are labelled `Web` and not persisted.
