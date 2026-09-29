//! Shared HTTP conventions for data plugins (port of `delta/core/http.py`).

/// `Delta/<version> (<extra>)`. The Python version reads the installed
/// distribution version; the Rust binary version is the workspace version.
pub fn user_agent(extra: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    if extra.is_empty() {
        format!("Delta/{version}")
    } else {
        format!("Delta/{version} ({extra})")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn user_agent_format() {
        assert_eq!(
            super::user_agent("research harness"),
            format!("Delta/{} (research harness)", env!("CARGO_PKG_VERSION"))
        );
    }
}
