"""Company research: browse stored sources and follow report citations."""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.widgets import Button, Input, Markdown, MarkdownViewer, Select, Static
from textual.widgets._markdown import MarkdownBlock

from rigger import services
from rigger.evidence import EvidenceItem, evidence, evidence_by_ids
from rigger.reports import _SECTIONS, Report, build_report, render_markdown, write_report
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneRow, RiggerTable


@dataclass
class CompanyView:
    search: str = ""
    kind: str = "all"
    limit: int = 200
    selected: str = ""
    inspected: str = ""
    report_y: float = 0
    list_y: float = 0
    preview_y: float = 0


@dataclass
class ResearchState:
    target: str = ""
    company: str = ""
    companies: dict[str, CompanyView] = field(default_factory=dict)
    busy: bool = False
    activity: str = ""


class ResearchViewer(MarkdownViewer):
    def on_show(self) -> None:
        self.screen.restore_report_position()

    async def _on_markdown_link_clicked(self, message: Markdown.LinkClicked) -> None:
        message.prevent_default()
        if message.href.startswith("evidence:"):
            message.stop()
            await self.screen.inspect_evidence(message.href.removeprefix("evidence:"))
        elif message.href.startswith(("https://", "http://")):
            message.stop()
            self.app.open_url(message.href)
        elif message.href.startswith("#"):
            await super()._on_markdown_link_clicked(message)
        else:
            message.stop()


class Research(RiggerScreen):
    CSS = """
    #research-header, #research-actions, #evidence-filters, #preview-actions { height: auto; }
    #research-header { max-height: 6; }
    #report-targets { height: 4; width: 1fr; }
    #research-company { width: 1fr; }
    #research-actions Button { min-width: 10; }
    #research-status { height: auto; color: $text-muted; }
    #evidence-layout, #report-doc { height: 1fr; }
    #evidence-list-pane { width: 2fr; }
    #evidence-preview-pane { width: 3fr; }
    #evidence-search { width: 1fr; }
    #evidence-kind { width: 20; }
    #evidence-table, #evidence-preview, #report-view { height: 1fr; }
    #evidence-empty, #evidence-count, #report-legacy { height: auto; }
    #source-body { height: auto; }
    """
    initial_tab = "evidence"

    def __init__(self, rig: Any, state: ResearchState | None = None) -> None:
        super().__init__(rig)
        self.state = state or ResearchState()
        self.tab = self.initial_tab
        self.report: Report | None = None
        self.items: dict[str, EvidenceItem] = {}
        self.ready = False
        self.detail_open = False
        self._rendered_company = ""
        self.status_text = ""
        self._claim_to_reveal = ""

    @property
    def view(self) -> CompanyView:
        return self.state.companies.setdefault(self.state.company, CompanyView())

    def compose_content(self) -> ComposeResult:
        with Horizontal(id="research-header"):
            yield RiggerTable(id="report-targets")
            yield Select([], prompt="Company", id="research-company")
        with Horizontal(id="research-actions"):
            yield Button("Evidence", id="tab-evidence")
            yield Button("Report", id="tab-report")
            yield Button("Gather all", id="research-gather")
            yield Button("Generate report", id="report-generate", variant="primary")
        yield Static("", id="research-status", markup=False)
        with PaneRow(id="evidence-layout"):
            with Pane(title="sources", id="evidence-list-pane"):
                with Horizontal(id="evidence-filters"):
                    yield Input(placeholder="Search sources", id="evidence-search")
                    yield Select(
                        [
                            (label, value)
                            for label, value in (
                                ("All", "all"),
                                ("News", "news"),
                                ("Filings", "filing"),
                                ("Prices", "bar"),
                                ("Fundamentals", "fundamental"),
                                ("Events", "event"),
                            )
                        ],
                        value="all",
                        allow_blank=False,
                        id="evidence-kind",
                    )
                yield RiggerTable(id="evidence-table")
                yield Static("", id="evidence-empty", markup=False)
                yield Static("", id="evidence-count", markup=False)
                yield Button("Load more", id="evidence-more")
            with Pane(title="preview", id="evidence-preview-pane"):
                with VerticalScroll(id="evidence-preview"):
                    yield Static("Select a source.", id="source-body", markup=False)
                with Horizontal(id="preview-actions"):
                    yield Button("Back", id="evidence-back")
                    yield Button("View in report", id="evidence-report")
                    yield Button("Open source", id="evidence-open")
        with Pane(title="report", id="report-doc"):
            yield Static("", id="report-legacy", markup=False)
            yield ResearchViewer(id="report-view", show_table_of_contents=True, open_links=False)

    async def on_mount(self) -> None:
        self.query_one("#report-targets", RiggerTable).add_columns("Watch target", "Kind")
        self.query_one("#evidence-table", RiggerTable).add_columns("Source", "Type", "Date")
        self.ready = True
        await self.refresh_view()

    def save_position(self) -> None:
        if self.ready and self._rendered_company:
            old = self.state.companies.setdefault(self._rendered_company, CompanyView())
            if self.tab == "report":
                old.report_y = self.query_one("#report-view", MarkdownViewer).scroll_y
            else:
                if self.query_one("#evidence-list-pane").display:
                    old.list_y = self.query_one("#evidence-table", RiggerTable).scroll_y
                if self.query_one("#evidence-preview-pane").display:
                    old.preview_y = self.query_one("#evidence-preview", VerticalScroll).scroll_y

    def on_screen_suspend(self) -> None:
        self.save_position()

    async def refresh_view(self) -> None:
        if not self.ready:
            return
        specs = sorted(services.target_specs().values(), key=lambda t: t.id)
        table = self.query_one("#report-targets", RiggerTable)
        with self.prevent(RiggerTable.RowHighlighted):
            table.clear()
            for target in specs:
                table.add_row(target.id, target.kind, key=target.id)
        ids = [target.id for target in specs]
        if self.state.target not in ids:
            self.state.target = ids[0] if ids else ""
        if self.state.target:
            with self.prevent(RiggerTable.RowHighlighted):
                table.move_cursor(row=ids.index(self.state.target))
        await self.select_target(self.state.target)

    async def select_target(self, target: str) -> None:
        self.state.target = target
        instruments = sorted(
            (i for i in self.rig.universe() if target in i.watchlists), key=lambda i: i.id
        )
        ids = [i.id for i in instruments]
        if self.state.company not in ids:
            self.state.company = ids[0] if ids else ""
        selector = self.query_one("#research-company", Select)
        with self.prevent(Select.Changed):
            selector.set_options(
                [(f"{getattr(i, 'name', '') or i.symbol} · {i.id}", i.id) for i in instruments]
            )
            selector.value = self.state.company or Select.NULL
        await self.load_company()

    async def load_company(self) -> None:
        self._rendered_company = self.state.company
        with self.prevent(Input.Changed, Select.Changed):
            self.query_one("#evidence-search", Input).value = self.view.search
            self.query_one("#evidence-kind", Select).value = self.view.kind
        self.query_one("#report-generate", Button).disabled = self.state.busy
        self.query_one("#research-gather", Button).disabled = self.state.busy
        self.detail_open = False
        await self.show_latest(self.state.company)
        self.load_evidence()
        if self.view.inspected:
            await self.inspect_evidence(self.view.inspected, save=False)
        self.show_tab(self.tab)
        self.call_after_refresh(self.restore_position)

    def restore_position(self) -> None:
        self.query_one("#report-view", MarkdownViewer).scroll_to(
            y=self.view.report_y, animate=False
        )
        self.query_one("#evidence-table", RiggerTable).scroll_to(y=self.view.list_y, animate=False)
        self.query_one("#evidence-preview", VerticalScroll).scroll_to(
            y=self.view.preview_y, animate=False
        )

    async def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        value = str(event.row_key.value)
        if event.data_table.id == "report-targets":
            if value != self.state.target:
                self.save_position()
                await self.select_target(value)
        elif value in self.items:
            self.view.selected = value
            self.view.inspected = ""
            self.preview(self.items[value])

    def on_data_table_row_selected(self, event: RiggerTable.RowSelected) -> None:
        if event.data_table.id == "evidence-table":
            self.detail_open = True
            self.layout_views()

    async def on_select_changed(self, event: Select.Changed) -> None:
        if not self.ready or event.value is Select.NULL:
            return
        if event.select.id == "research-company" and str(event.value) != self.state.company:
            self.save_position()
            self.state.company = str(event.value)
            await self.load_company()
        elif event.select.id == "evidence-kind" and str(event.value) != self.view.kind:
            self.view.kind = str(event.value)
            self.view.limit = 200
            self.view.inspected = ""
            self.load_evidence()

    def on_input_changed(self, event: Input.Changed) -> None:
        if self.ready and event.input.id == "evidence-search" and event.value != self.view.search:
            self.view.search = event.value
            self.view.limit = 200
            self.view.inspected = ""
            self.load_evidence()

    def load_evidence(self) -> None:
        rows = (
            evidence(
                self.rig.engine,
                target=self.state.company,
                kind=None if self.view.kind == "all" else self.view.kind,
                search=self.view.search,
                limit=self.view.limit + 1,
            )
            if self.state.company
            else []
        )
        self.query_one("#evidence-more", Button).disabled = len(rows) <= self.view.limit
        self.items = {item.id: item for item in rows[: self.view.limit]}
        table = self.query_one("#evidence-table", RiggerTable)
        with self.prevent(RiggerTable.RowHighlighted):
            table.clear()
            for item in self.items.values():
                table.add_row(item.title, item.kind, item.ts.strftime("%Y-%m-%d"), key=item.id)
        self.query_one("#evidence-count", Static).update(f"{len(self.items)} sources shown")
        self.query_one("#evidence-empty", Static).update(
            "" if rows else "No matching evidence. Gather all or change filters."
        )
        selected = self.items.get(self.view.selected) or next(iter(self.items.values()), None)
        if selected:
            self.view.selected = selected.id
            with self.prevent(RiggerTable.RowHighlighted):
                table.move_cursor(row=list(self.items).index(selected.id))
        self.preview(selected)

    def cited_ids(self) -> set[str]:
        return (
            {
                eid
                for field, _ in _SECTIONS
                for claim in getattr(self.report, field)
                for eid in claim.evidence_ids
            }
            if self.report
            else set()
        )

    def preview(self, item: EvidenceItem | None) -> None:
        self.query_one("#evidence-open", Button).disabled = not (
            item and item.url and item.url.startswith(("https://", "http://"))
        )
        self.query_one("#evidence-report", Button).disabled = not (
            item and item.id in self.cited_ids()
        )
        if item is None:
            self.query_one("#source-body", Static).update("Select a source.")
            return
        cited = ("Yes" if item.id in self.cited_ids() else "No") if self.report else "Unavailable"
        content = (
            item.body
            or "\n".join(f"{key}: {value}" for key, value in item.raw.items())
            or "No source text available."
        )
        if not item.body and item.kind in ("news", "filing"):
            content = "No source text available.\n\n" + content
        self.query_one("#source-body", Static).update(
            f"{item.title}\n\n{item.kind} · {item.source}\n{item.ts.isoformat()}\n\n{content}\n\n"
            f"Cited in latest report: {cited}\n{item.url or ''}"
        )

    async def show_latest(self, target_id: str) -> None:
        company = target_id
        if company != self.state.company:
            company = next(
                (i.id for i in self.rig.universe() if target_id in i.watchlists), company
            )
        paths = (
            sorted((Path(getattr(self.rig.cfg, "reports_dir", "reports")) / company).glob("*.md"))
            if company
            else []
        )
        self.report = None
        legacy = ""
        body = "# Research\n\nNo report yet. Select a company and Generate report."
        if paths:
            path = paths[-1]
            body = path.read_text(encoding="utf-8")
            try:
                report = Report.model_validate_json(
                    path.with_suffix(".json").read_text(encoding="utf-8")
                )
                if report.target_id != company or render_markdown(report) != body:
                    raise ValueError("Sidecar does not match Markdown")
                self.report = report
                body = render_markdown(report, interactive=True)
            except (OSError, ValueError):
                legacy = "Regenerate this report to enable interactive citations."
        self.query_one("#report-legacy", Static).update(legacy)
        await self.query_one("#report-view", MarkdownViewer).document.update(body)
        self.query_one("#report-doc", Pane).set_badge(paths[-1].stem if paths else "no report")
        prices = evidence(self.rig.engine, target=company, kind="bar", limit=1) if company else []
        price_date = prices[0].ts.strftime("%Y-%m-%d %H:%M UTC") if prices else "none"
        self.status_text = f"{company or 'Select a watch target and company'} · report: {paths[-1].stem if paths else 'none'} · latest price: {price_date}"
        self.query_one("#research-status", Static).update(self.state.activity or self.status_text)

    async def inspect_evidence(self, evidence_id: str, *, save: bool = True) -> None:
        if save:
            self.save_position()
        self.view.inspected = evidence_id
        found = evidence_by_ids(self.rig.engine, [evidence_id])
        if save:
            self.show_tab("evidence")
        self.detail_open = True
        self.layout_views()
        if found:
            self.items[found[0].id] = found[0]
            self.view.selected = found[0].id
            self.preview(found[0])
        else:
            self.preview(None)
            self.query_one("#source-body", Static).update(
                f"Source no longer available.\n\n{self.report.citations.get(evidence_id, evidence_id) if self.report else evidence_id}"
            )
        self.query_one("#evidence-preview", VerticalScroll).scroll_home(animate=False)

    def show_tab(self, tab: str) -> None:
        self.tab = tab
        self.query_one("#tab-evidence", Button).variant = (
            "primary" if tab == "evidence" else "default"
        )
        self.query_one("#tab-report", Button).variant = "primary" if tab == "report" else "default"
        self.layout_views()

    def layout_views(self) -> None:
        narrow = self.size.width < 100
        self.query_one("#evidence-layout").display = self.tab == "evidence"
        self.query_one("#report-doc").display = self.tab == "report"
        self.query_one("#evidence-list-pane").display = not (narrow and self.detail_open)
        self.query_one("#evidence-preview-pane").display = not narrow or self.detail_open
        self.query_one("#evidence-back").display = narrow

    def on_resize(self) -> None:
        if self.ready:
            self.layout_views()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        action = event.button.id
        if action in ("tab-evidence", "tab-report"):
            self.save_position()
            self.show_tab(action.removeprefix("tab-"))
            self.call_after_refresh(self.restore_position)
        elif action == "evidence-more":
            self.view.limit += 200
            self.load_evidence()
        elif action == "evidence-back":
            self.view.inspected = ""
            self.load_evidence()
            self.detail_open = False
            self.layout_views()
        elif action == "evidence-report":
            self._claim_to_reveal = self.view.selected
            self.show_tab("report")
        elif action == "evidence-open":
            item = self.items.get(self.view.selected)
            if item and item.url and item.url.startswith(("https://", "http://")):
                self.app.open_url(item.url)
        elif action == "report-generate":
            if not self.state.company:
                self.notify("Select a company first", severity="error")
            elif not self.state.busy:
                self.generate(self.state.company)
        elif action == "research-gather" and not self.state.busy:
            self.gather_all()

    def restore_report_position(self) -> None:
        # Show arrives after layout gives the previously hidden document a region.
        if not self.ready or self.tab != "report":
            return
        if self._claim_to_reveal:
            self.scroll_to_claim()
        else:
            self.query_one("#report-view", MarkdownViewer).scroll_to(
                y=self.view.report_y, animate=False, immediate=True
            )

    def scroll_to_claim(self) -> None:
        """Locate the first citing claim by its rendered source line."""
        document = self.query_one("#report-view", MarkdownViewer).document
        lines = document.source.splitlines()
        marker = f"(evidence:{self._claim_to_reveal or self.view.selected})"
        citation_line = next((i for i, line in enumerate(lines) if marker in line), None)
        if citation_line is None:
            return
        claim_line = next(
            (i for i in range(citation_line - 1, -1, -1) if lines[i].startswith("- ")),
            citation_line,
        )
        blocks = [
            block
            for block in document.query(MarkdownBlock)
            if block.source_range[0] <= claim_line < block.source_range[1]
        ]
        if blocks:
            min(blocks, key=lambda b: b.source_range[1] - b.source_range[0]).scroll_visible(
                top=True, animate=False, immediate=True
            )
            self._claim_to_reveal = ""

    def research_screens(self) -> list[Research]:
        screens = [
            screen
            for screen in getattr(self.app, "_screens", {}).values()
            if isinstance(screen, Research) and screen.ready
        ]
        return screens or [self]

    def set_busy(self, busy: bool, label: str = "") -> None:
        self.state.busy = busy
        self.state.activity = label if busy else ""
        for screen in self.research_screens():
            screen.query_one("#report-generate", Button).disabled = busy
            screen.query_one("#research-gather", Button).disabled = busy
            screen.query_one("#research-status", Static).update(
                self.state.activity or screen.status_text
            )

    @work
    async def generate(self, company: str) -> None:
        self.set_busy(True, f"Generating report for {company}…")
        try:
            report = await build_report(self.rig, company)
            write_report(report, Path(getattr(self.rig.cfg, "reports_dir", "reports")))
            self.notify(f"Report written for {company}")
            for screen in self.research_screens():
                if screen._rendered_company == company and self.state.company == company:
                    await screen.show_latest(company)
                    screen.preview(screen.items.get(screen.view.selected))
        except Exception as exc:
            self.notify(f"Report failed: {exc}", severity="error")
        finally:
            self.set_busy(False)

    @work
    async def gather_all(self) -> None:
        self.set_busy(True, "Gathering all configured targets…")
        try:
            await services.ingest(self.rig)
            self.set_busy(True, "Extracting events from gathered evidence…")
            await services.extract(self.rig)
            for screen in self.research_screens():
                if screen._rendered_company == self.state.company:
                    if screen.is_current:
                        screen.save_position()
                    await screen.show_latest(self.state.company)
                    screen.load_evidence()
                    screen.call_after_refresh(screen.restore_position)
            self.notify("Evidence refreshed")
        except Exception as exc:
            self.notify(f"Gather failed: {exc}", severity="error")
        finally:
            self.set_busy(False)
