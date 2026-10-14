//! LLM layer for Delta: providers, client, cache, cost log, routing,
//! structured calls and the citation-validity harness (R1c).

pub mod catalog;
pub mod client;
pub mod eval;
pub mod fake;
pub mod jev;
pub mod json;
pub mod prompts;
pub mod providers;
pub mod router;
pub mod structured;

pub use catalog::{
    cached_catalog, catalog, catalog_path, read_cache_entry, set_llm_custom, set_llm_model,
    set_llm_provider, set_llm_route, set_plugin_model, write_cache, CACHE_TTL_SECONDS,
    CATALOG_FILENAME,
};
pub use client::{build_client, LlmClient, LlmResult, SYSTEM_PROMPT};
pub use eval::{
    citation_validity_rate, evaluate, hallucinated_citations, valid_citations, EvalResult,
    GoldenCase,
};
pub use fake::{CannedResponse, FakeLlm, RecordedCall};
pub use jev::{
    ChoiceQuestion, Decision, JevClient, JevError, Usage, DECISIONS_URL, JEV_MODEL, PROMPT_VERSION,
};
pub use json::extract_json;
pub use providers::{
    provider_spec, verify_key, CompletionRequest, Message, ModelInfo, OpenAiCompatProvider,
    OpenRouterProvider, Provider, ProviderError, ProviderResult, ProviderSpec, PROVIDERS,
};
pub use router::model_for;
pub use structured::structured;
