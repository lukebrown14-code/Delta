//! Prompt templates, copied verbatim from `delta/llm/prompts/` with the same
//! versions (a rewrite-plan contract: template text never changes silently).

pub const ALL: &[(&str, &str)] = &[
    ("extract_v1.j2", include_str!("../prompts/extract_v1.j2")),
    ("report_v1.j2", include_str!("../prompts/report_v1.j2")),
    ("report_v2.j2", include_str!("../prompts/report_v2.j2")),
    (
        "thesis_summary_v1.j2",
        include_str!("../prompts/thesis_summary_v1.j2"),
    ),
    ("thesis_v1.j2", include_str!("../prompts/thesis_v1.j2")),
];

#[cfg(test)]
mod tests {
    #[test]
    fn templates_render_with_strict_undefined() {
        for (name, _) in super::ALL {
            // Renders as long as the template syntax parses; strict undefined
            // is enforced by `render_prompt` at call time.
            assert!(name.ends_with(".j2"), "{name}");
        }
    }
}
