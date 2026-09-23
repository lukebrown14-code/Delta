"""Grounded Q&A over the stored evidence pool, with web search as an opt-in tool.

Local data first: the model's context is the evidence gathered for the selected
targets, rendered with ids and cite text. When the user allows web search the
model may request queries through a :class:`SearchTool`; the hits reach the
model labelled as Web evidence and are never written to the stored pool. Every
citation in the returned answer is machine-checked against the gathered
evidence, and answers without verifiable support are labelled AI inference.
"""

from __future__ import annotations

import json
from typing import Any, Literal, Protocol

from pydantic import BaseModel, Field, TypeAdapter, ValidationError
from sqlalchemy.engine import Engine

from delta.evidence import EvidenceItem, cite, evidence
from delta.llm.router import model_for

ChatSource = Literal["stored", "web", "inference", "user"]

CHAT_PROMPT_VERSION = "chat_v1"

UNSUPPORTED_NOTE = (
    "This answer could not be supported from your stored data; treat it as AI inference."
)

_CHAT_EVIDENCE_LIMIT = 50

_CHAT_SYSTEM_PROMPT = """You are Delta, a research assistant answering questions about the user's watch
targets using only the evidence supplied in this conversation.

Rules:
- Use only the supplied evidence. Never reason from memory; when the evidence is
  insufficient, say what is missing instead of guessing.
- Support every fact with a citation: the id of a stored evidence item (for example
  "bar:5") or the url of a web result.
- Stored evidence is established fact. Web results are unverified context and must be
  treated as such.
- Do not recommend buying or selling anything.

Reply with JSON only, no surrounding text:
{"answer": "<your reply>", "citations": ["<evidence id or web url>", ...], "web_queries": [...]}"""

_CHAT_WEB_RULE = """Web search is enabled: web_queries lists at most three short search queries that
would help answer the question; leave it empty when none are needed."""

_CHAT_NO_WEB_RULE = "Web search is disabled: web_queries must always be an empty list."


class ChatMessage(BaseModel):
    """One conversation turn; citations are stored evidence ids or web result urls."""

    role: Literal["user", "assistant", "tool"]
    text: str
    citations: tuple[str, ...] = ()
    source: ChatSource


class WebHit(BaseModel):
    """One result from the opt-in web search tool; never persisted as evidence."""

    title: str
    url: str
    snippet: str = ""


class ChatDraft(BaseModel):
    """The model's machine-checkable answer contract."""

    answer: str
    citations: list[str] = Field(default_factory=list)
    web_queries: list[str] = Field(default_factory=list)


class SearchTool(Protocol):
    """Provider-agnostic web search behind the chat's explicit, user-gated tool."""

    async def search(self, query: str) -> list[WebHit]: ...


class OfflineSearchTool:
    """No-op default: the real search provider is a later, separately-gated plugin."""

    async def search(self, query: str) -> list[WebHit]:
        return []


_RESPONSE_FORMAT: dict[str, Any] = {
    "type": "json_schema",
    "json_schema": {"name": "ChatDraft", "schema": ChatDraft.model_json_schema()},
}

_ADAPTER: TypeAdapter[ChatDraft] = TypeAdapter(ChatDraft)


async def chat(
    delta: Any,
    history: list[ChatMessage],
    *,
    targets: list[str],
    allow_web: bool = False,
    search: SearchTool | None = None,
) -> ChatMessage:
    """Answer the last user message in ``history`` from evidence gathered for ``targets``.

    The model is offered the stored evidence for each target (ids plus cite
    text); with ``allow_web`` it may request searches through ``search`` in one
    extra model round, and those hits are never persisted. Returned citations
    are verified against the gathered evidence: stored ids and web urls are
    kept, anything else is dropped and the answer is labelled inference.
    """
    model = model_for(delta.cfg, "chat")
    tool: SearchTool = search if search is not None else OfflineSearchTool()
    items = _gather_evidence(delta.engine, targets)
    hits: list[WebHit] = []
    head: list[dict[str, str]] = [
        {"role": "system", "content": _system_prompt(allow_web)},
        {"role": "user", "content": _stored_context(items)},
    ]
    draft = await _draft(delta.llm, model, head + _history_messages(history))
    if allow_web and draft.web_queries:
        hits = await _search(tool, draft.web_queries)
        if hits:
            head.append({"role": "user", "content": _web_context(hits)})
            draft = await _draft(delta.llm, model, head + _history_messages(history))
    return _verified(draft, items, hits)


async def _draft(llm: Any, model: str, messages: list[dict[str, str]]) -> ChatDraft:
    """One chat-path call returning a validated ChatDraft, with one retry on bad JSON."""
    result = await llm.chat(
        task="chat",
        model=model,
        prompt_version=CHAT_PROMPT_VERSION,
        messages=messages,
        response_format=_RESPONSE_FORMAT,
    )
    try:
        return _parse_draft(result.text)
    except (ValidationError, json.JSONDecodeError) as exc:
        retry = messages + [
            {
                "role": "user",
                "content": (
                    "Your previous reply was not valid JSON for the required schema."
                    f" Return valid JSON only, fixing: {exc}"
                ),
            }
        ]
        result = await llm.chat(
            task="chat",
            model=model,
            prompt_version=CHAT_PROMPT_VERSION,
            messages=retry,
            response_format=_RESPONSE_FORMAT,
        )
        return _parse_draft(result.text)


async def _search(tool: SearchTool, queries: list[str]) -> list[WebHit]:
    """Run every requested query once, keeping first-seen order and dropping duplicate urls."""
    hits: list[WebHit] = []
    seen: set[str] = set()
    for query in queries:
        for hit in await tool.search(query):
            if hit.url not in seen:
                seen.add(hit.url)
                hits.append(hit)
    return hits


def _verified(draft: ChatDraft, items: list[EvidenceItem], hits: list[WebHit]) -> ChatMessage:
    """Check the draft's citations: stored ids and web urls are kept, the rest dropped.

    Source precedence: any dropped citation forces inference; otherwise any web
    citation forces web; otherwise the answer is stored. An answer with no
    verifiable citations keeps none and gains the unsupported note.
    """
    stored = {item.id for item in items}
    web = {hit.url for hit in hits}
    verified: list[str] = []
    dropped = False
    used_web = False
    for citation in draft.citations:
        if citation in stored:
            verified.append(citation)
        elif citation in web:
            verified.append(citation)
            used_web = True
        else:
            dropped = True
    text = draft.answer
    if not verified:
        text = f"{text.rstrip()}\n\n{UNSUPPORTED_NOTE}"
    if not verified or dropped:
        source: ChatSource = "inference"
    elif used_web:
        source = "web"
    else:
        source = "stored"
    return ChatMessage(
        role="assistant",
        text=text,
        citations=tuple(dict.fromkeys(verified)),
        source=source,
    )


def _gather_evidence(engine: Engine, targets: list[str]) -> list[EvidenceItem]:
    """Evidence for each target, newest first per target, de-duplicated by id."""
    by_id: dict[str, EvidenceItem] = {}
    for target in targets:
        for item in evidence(engine, target=target, limit=_CHAT_EVIDENCE_LIMIT):
            by_id.setdefault(item.id, item)
    return list(by_id.values())


def _history_messages(history: list[ChatMessage]) -> list[dict[str, str]]:
    """Flatten ChatMessages to provider messages; tool turns become labelled user turns."""
    messages: list[dict[str, str]] = []
    for msg in history:
        text = msg.text
        if msg.citations:
            text += "\n" + " ".join(f"({citation})" for citation in msg.citations)
        if msg.role == "tool":
            text = f"[web search result] {text}"
        role = "assistant" if msg.role == "assistant" else "user"
        messages.append({"role": role, "content": text})
    return messages


def _system_prompt(allow_web: bool) -> str:
    rule = _CHAT_WEB_RULE if allow_web else _CHAT_NO_WEB_RULE
    return f"{_CHAT_SYSTEM_PROMPT}\n\n{rule}"


def _stored_context(items: list[EvidenceItem]) -> str:
    """The local-first context: every gathered item with its id and cite text."""
    lines = ["Stored evidence for the selected targets (cite items by id):"]
    if not items:
        lines.append("(no stored evidence for these targets)")
    for item in items:
        line = f"- {item.id}: {cite(item)}"
        if item.body:
            line += f" — {' '.join(item.body.split())[:200]}"
        lines.append(line)
    return "\n".join(lines)


def _web_context(hits: list[WebHit]) -> str:
    """Web hits labelled as unverified Web evidence, cited by url, never stored."""
    lines = ["Web search results (not stored; unverified context, cite by url):"]
    for hit in hits:
        lines.append(f"- {hit.url}: {hit.title} — {hit.snippet}")
    return "\n".join(lines)


def _parse_draft(text: str) -> ChatDraft:
    return _ADAPTER.validate_python(_load_json(text))


def _load_json(text: str) -> Any:
    """json.loads after stripping a markdown code fence, mirroring structured outputs."""
    stripped = text.strip()
    if stripped.startswith("```"):
        body = stripped.split("```", 2)[1]
        if body.startswith("json"):
            body = body[4:]
        stripped = body.strip()
    return json.loads(stripped)
