//! In-process event bus. Port of `delta/core/events.py`.
//!
//! Finding (rust-core): the Python bus is async (asyncio handlers). The Rust
//! port uses sync handlers called inline from [`EventBus::emit`]; the pipeline
//! callers are synchronous in the services crate, so this preserves
//! behaviour (fan-out, handler errors logged and swallowed) without pulling
//! tokio into delta-core. Revisit when an async subscriber arrives.

use std::collections::HashMap;

use serde_json::Value;

type EventHandler = Box<dyn Fn(&str, &HashMap<String, Value>) + Send + Sync>;

/// Fan-out bus: subscribers register per event name; a failing handler is
/// logged and skipped, never propagated to the emitter.
#[derive(Default)]
pub struct EventBus {
    subscribers: HashMap<String, Vec<EventHandler>>,
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn subscribe<F>(&mut self, event: &str, handler: F)
    where
        F: Fn(&str, &HashMap<String, Value>) + Send + Sync + 'static,
    {
        self.subscribers
            .entry(event.to_string())
            .or_default()
            .push(Box::new(handler));
    }

    pub fn emit(&self, event: &str, payload: HashMap<String, Value>) {
        if let Some(handlers) = self.subscribers.get(event) {
            for handler in handlers {
                // Subscribers must not break the pipeline.
                if let Err(err) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handler(event, &payload)
                })) {
                    log::error!("event handler panicked for {event}: {err:?}");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fan_out_and_isolation() {
        let mut bus = EventBus::new();
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let h = hits.clone();
        bus.subscribe("gather.done", move |_, _| {
            h.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        bus.subscribe("gather.done", |_, _| panic!("boom"));
        bus.emit(
            "gather.done",
            HashMap::from([("count".to_string(), Value::from(3))]),
        );
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        bus.emit("other", HashMap::new()); // no subscriber: fine
    }
}
