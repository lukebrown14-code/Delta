# Delta

A personal investment research assistant: the user names what they're
interested in, Delta gathers evidence from public sources, an AI synthesises
cited reports and grounded chat, and an optional thesis layer tracks
long-horizon ideas. The product is clarity, not trading signals.

## Language

### Watching

**Target**:
A thing the user has named for Delta to follow: a company, sector, industry, market, or theme.
_Avoid_: watchlist (legacy name for the same id), topic, subject

**Instrument**:
A tradable security identified as market + symbol, e.g. `US:AAPL`.
_Avoid_: ticker, stock, symbol

**Market**:
A trading venue convention (e.g. US, ASX) that namespaces symbols and sets the reporting currency.
_Avoid_: exchange

**Theme**:
A target kind spanning multiple companies around one idea rather than a single entity.
_Avoid_: sector (a sector is a formal classification; a theme is a user's idea)

### Evidence

**Evidence**:
A single gathered, citable fact or observation about a target, addressable by a stable id.
_Avoid_: data point, document, article

**Evidence pool**:
The unified set of all gathered evidence; the only facts a model may reason from.

**Source record**:
A raw row written by a data plugin — bar, news item, fundamental, event — flattened into evidence for reading.

**Event**:
A structured fact extracted from a news item (e.g. an earnings beat, a guidance change).
_Avoid_: news (news is the raw item; an event is what was extracted from it)

**Candidate evidence**:
Evidence a thesis proposes for its case, held unaccepted until the user accepts it.
_Avoid_: linked evidence, suggested evidence

**Accepted evidence**:
Candidate evidence the user has accepted; the only input to thesis health.

**Web result**:
A search hit surfaced during chat as a labelled tool result. It supports the answer but is never stored as evidence.
_Avoid_: web evidence

### Synthesis

**Report**:
A cited, both-sides synthesis of gathered evidence for one target: summary, bull, bear, risks, catalysts, unknowns.
_Avoid_: analysis, summary

**Brief**:
A facts-only, no-model summary of current state for one instrument.
_Avoid_: report

**Citation**:
The link from a claim to the evidence id it rests on; machine-checked against the pool.
_Avoid_: reference, source

**Citation contract**:
The rule that claims citing ungathered evidence are dropped and a draft left with no substantive claims is rejected.

**Bull / Bear**:
The paired arguments for and against; both are always present in a report.

**AI inference**:
The label a chat answer carries when it lacks verifiable support, so it is never presented as fact.
_Avoid_: hallucination, guess

### Theses

**Thesis**:
A long-horizon claim the user tracks as evidence accumulates for and against it.
_Avoid_: hypothesis, idea, position, conviction

**Thesis health**:
The deterministic state of a thesis, computed by a pure function over its accepted evidence.
_Avoid_: score, rating

**Health state**:
One of emerging, building, mixed, weakening, challenged, idle.

### Pipeline

**Ingest**:
Data plugins fetching source records into the local store.

**Extraction**:
Turning news items into structured events.

**Gather evidence**:
The user action that runs ingest and extraction together.
