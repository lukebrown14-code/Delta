//! LLM layer for Delta: providers, client, cache, cost log, routing,
//! structured calls and the citation-validity harness (R1c).

pub mod client;
pub mod eval;
pub mod json;
pub mod prompts;
pub mod providers;
pub mod router;
pub mod structured;

pub use client::{build_client, LlmClient, LlmResult, SYSTEM_PROMPT};
pub use eval::{
    citation_validity_rate, evaluate, hallucinated_citations, valid_citations, EvalResult,
    GoldenCase,
};
pub use json::extract_json;
pub use providers::{
    provider_spec, verify_key, CompletionRequest, Message, ModelInfo, OpenAiCompatProvider,
    OpenRouterProvider, Provider, ProviderError, ProviderResult, ProviderSpec, PROVIDERS,
};
pub use router::model_for;
pub use structured::structured;
