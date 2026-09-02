use super::{first_h1, scan_map_body};

fn destination(body: &str) -> Option<String> {
    scan_map_body(body).destination
}

#[test]
fn destination_extracts_first_paragraph_under_the_heading() {
    let body = "## Destination\n\nAll eight issues merged\nand usable in the TUI.\n\nSecond paragraph.\n\n## Notes\n";
    assert_eq!(
        destination(body).as_deref(),
        Some("All eight issues merged and usable in the TUI.")
    );
    assert_eq!(
        destination("# Destination\n\nAny heading level works.\n").as_deref(),
        Some("Any heading level works."),
        "real maps drift from the template's ## level"
    );
    assert_eq!(destination("no heading here"), None);
    assert_eq!(
        destination("## Destination\n\n## Notes\n"),
        None,
        "an empty section yields None, not an empty string"
    );
}

#[test]
fn destination_heading_recognition_is_real_markdown() {
    assert_eq!(
        destination("## Destination ##\n\nClosed ATX heading.\n").as_deref(),
        Some("Closed ATX heading.")
    );
    assert_eq!(
        destination("Destination\n-----------\n\nSetext heading.\n").as_deref(),
        Some("Setext heading.")
    );
    assert_eq!(
        destination("```\n# Destination\n\nnot a destination\n```\n"),
        None,
        "a heading inside a code fence is code, not a heading"
    );
}

#[test]
fn task_list_detection_ignores_code_fences() {
    assert!(scan_map_body("- [ ] #3 open thing\n").has_task_list);
    assert!(
        !scan_map_body("```\n- [ ] #3 fenced example\n```\n").has_task_list,
        "a task list inside a code fence is code, not tickets"
    );
}

#[test]
fn first_h1_reads_heading_text_not_slug_or_deeper_headings() {
    assert_eq!(
        first_h1("---\nnot: frontmatter to markdown\n---\n\n# Real title\n\n## Question\n")
            .as_deref(),
        // A leading `---` block renders as a setext H2 ("not: frontmatter...")
        // or thematic break, never an H1, so the real title still wins.
        Some("Real title")
    );
    assert_eq!(first_h1("## Only a section heading\n"), None);
    assert_eq!(first_h1("prose only"), None);
    assert_eq!(
        first_h1("```\n# fenced\n```\n\n# Real\n").as_deref(),
        Some("Real"),
        "a heading inside a code fence is code, not a title"
    );
}
