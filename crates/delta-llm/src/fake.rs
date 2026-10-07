//! Offline LLM test double: canned responses from `fixtures/llm/`.
//!
//! The fixture format is the R3.0/R3.2 contract (`tests/golden_seed.py`,
//! amended 4 Oct): `fixtures/llm/<task>/<name>.json` =
//! `{"cost_usd": <float>, "text": <payload>}`, where `<task>/response.json`
//! is the task's default, addressable as `<task>`, and other files load
//! under `<task>/<name>`. This mirrors `load_llm_fixtures` on the Python
//! side, so both ecosystems read the same files.
//!
//! R3.2 chat/report streams inject it through the [`Provider`] trait object
//! of a real [`LlmClient`](crate::client::LlmClient): the canned `cost_usd`
//! flows through the normal cost logging, and the payload arrives as the
//! completion text exactly as the Python `FakeLLM` serialises it.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;

use crate::providers::{CompletionRequest, Message, Provider, ProviderError, ProviderResult};

/// One canned response file: `{"cost_usd": float, "text": payload}`.
#[derive(Debug, Clone, Deserialize)]
pub struct CannedResponse {
    pub cost_usd: f64,
    pub text: Value,
}

/// A recorded `complete` call, for assertions in the consuming stream's
/// tests (the Python `FakeLLM` records the same shape).
#[derive(Debug, Clone)]
pub struct RecordedCall {
    pub model: String,
    pub messages: Vec<Message>,
}

/// Trait-object test double: serve fixture-keyed canned responses.
///
/// `serve("report/apple")` selects the response for subsequent calls (sticky
/// until changed) — the same per-flow swap the Python seed rig performs by
/// replacing `rig.llm`.
pub struct FakeLlm {
    fixtures: BTreeMap<String, CannedResponse>,
    current: Mutex<String>,
    calls: Mutex<Vec<RecordedCall>>,
}

impl FakeLlm {
    /// Load every `*.json` under `root` (`fixtures/llm`), keyed
    /// `<task>` for `<task>/response.json` and `<task>/<name>` otherwise.
    pub fn load(root: &Path) -> Result<Self, String> {
        let mut fixtures = BTreeMap::new();
        let mut task_dirs: Vec<_> = std::fs::read_dir(root)
            .map_err(|e| format!("llm fixtures root {}: {e}", root.display()))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        task_dirs.sort_by_key(|e| e.file_name());
        for task_dir in task_dirs {
            let task = task_dir.file_name().to_string_lossy().to_string();
            let mut files: Vec<_> = std::fs::read_dir(task_dir.path())
                .map_err(|e| format!("llm fixtures {task}: {e}"))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
                .collect();
            files.sort_by_key(|e| e.file_name());
            for file in files {
                let name = file
                    .path()
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let text = std::fs::read_to_string(file.path())
                    .map_err(|e| format!("{}: {e}", file.path().display()))?;
                let canned: CannedResponse = serde_json::from_str(&text)
                    .map_err(|e| format!("{}: {e}", file.path().display()))?;
                let key = if name == "response" {
                    task.clone()
                } else {
                    format!("{task}/{name}")
                };
                fixtures.insert(key, canned);
            }
        }
        let current = fixtures
            .keys()
            .next()
            .cloned()
            .ok_or_else(|| format!("no llm fixtures under {}", root.display()))?;
        Ok(Self {
            fixtures,
            current: Mutex::new(current),
            calls: Mutex::new(Vec::new()),
        })
    }

    /// Serve `key` ("task" or "task/name") for the following calls.
    pub fn serve(&self, key: &str) {
        *self.current.lock().unwrap() = key.to_string();
    }

    /// Calls seen so far, oldest first.
    pub fn calls(&self) -> Vec<RecordedCall> {
        self.calls.lock().unwrap().clone()
    }

    /// The canned `cost_usd` of `key`, without a call.
    pub fn cost_of(&self, key: &str) -> Option<f64> {
        self.fixtures.get(key).map(|c| c.cost_usd)
    }
}

#[async_trait::async_trait]
impl Provider for FakeLlm {
    fn name(&self) -> &'static str {
        "fake"
    }

    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        self.calls.lock().unwrap().push(RecordedCall {
            model: req.model.to_string(),
            messages: req.messages.to_vec(),
        });
        let current = self.current.lock().unwrap().clone();
        let canned = self
            .fixtures
            .get(&current)
            .ok_or_else(|| ProviderError::Status {
                status: 0,
                body: format!("no llm fixture for key {current:?}"),
            })?;
        // The payload is the model's textual output, serialised verbatim
        // (Python `FakeLLM`: `text=json.dumps(payload)`).
        let text = serde_json::to_string(&canned.text).map_err(|e| ProviderError::Status {
            status: 0,
            body: format!("fixture {current:?}: {e}"),
        })?;
        Ok(ProviderResult {
            text,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: canned.cost_usd,
        })
    }
}
