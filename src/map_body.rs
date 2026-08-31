//! Wayfinder map/ticket body scanning shared by the GitHub and local
//! transports. The map template (a `Destination` section; task-list bodies
//! as the GitHub fallback signal) is a wayfinder convention, not a
//! source-specific one, so both transports read bodies through the same
//! `pulldown-cmark` scan — the renderer's own parser, so heading and fence
//! recognition can't drift from what the user sees.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// What one Markdown parse of a map body yields: the Destination one-liner
/// and the task-list half of the GitHub fallback signal (spec §4.4).
pub struct MapBodyFacts {
    /// The first paragraph under a `Destination` heading (any heading level,
    /// ATX or setext — the wayfinder template says `##`, real maps drift),
    /// wrapped lines joined. `None` when the body has no such heading or the
    /// section is empty.
    pub destination: Option<String>,
    /// The body carries a GitHub task list (a real one — a `- [ ]` line
    /// inside a code fence is not a task list).
    pub has_task_list: bool,
}

/// Scan a map body once.
pub fn scan_map_body(body: &str) -> MapBodyFacts {
    enum DestinationScan {
        SearchingForHeading,
        CollectingHeadingText(String),
        AwaitingParagraph,
        CapturingParagraph(String),
        Done(Option<String>),
    }
    use DestinationScan::*;

    let mut scan = SearchingForHeading;
    let mut has_task_list = false;
    for event in Parser::new_ext(body, Options::ENABLE_TASKLISTS) {
        if matches!(event, Event::TaskListMarker(_)) {
            has_task_list = true;
        }
        scan = match (scan, &event) {
            (SearchingForHeading, Event::Start(Tag::Heading { .. })) => {
                CollectingHeadingText(String::new())
            }
            (CollectingHeadingText(mut text), Event::Text(t) | Event::Code(t)) => {
                text.push_str(t);
                CollectingHeadingText(text)
            }
            (CollectingHeadingText(text), Event::End(TagEnd::Heading(_))) => {
                if text.trim().eq_ignore_ascii_case("destination") {
                    AwaitingParagraph
                } else {
                    SearchingForHeading
                }
            }
            // A heading before any paragraph: the Destination section is empty.
            (AwaitingParagraph, Event::Start(Tag::Heading { .. })) => Done(None),
            (AwaitingParagraph, Event::Start(Tag::Paragraph)) => CapturingParagraph(String::new()),
            (CapturingParagraph(mut text), Event::Text(t) | Event::Code(t)) => {
                text.push_str(t);
                CapturingParagraph(text)
            }
            (CapturingParagraph(mut text), Event::SoftBreak | Event::HardBreak) => {
                text.push(' ');
                CapturingParagraph(text)
            }
            (CapturingParagraph(text), Event::End(TagEnd::Paragraph)) => {
                let trimmed = text.trim();
                Done((!trimmed.is_empty()).then(|| trimmed.to_owned()))
            }
            (state, _) => state,
        };
    }
    MapBodyFacts {
        destination: match scan {
            Done(destination) => destination,
            _ => None,
        },
        has_task_list,
    }
}

/// The text of the first H1 heading — a local map's or ticket file's title
/// (never the filename slug: slugs drift after rescopes). `None` when the
/// body has no H1.
pub fn first_h1(body: &str) -> Option<String> {
    let mut text: Option<String> = None;
    for event in Parser::new_ext(body, Options::empty()) {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => text = Some(String::new()),
            Event::Text(t) | Event::Code(t) => {
                if let Some(text) = text.as_mut() {
                    text.push_str(&t);
                }
            }
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => {
                let title = text.take().unwrap_or_default();
                let title = title.trim();
                if !title.is_empty() {
                    return Some(title.to_owned());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests;
