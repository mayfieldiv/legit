use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use super::table::{Column, Columns, FillReserves, Table};
use crate::{
    app::grouping::{DisplayRow, Grouping},
    app::model::Model,
    blocker::{BlockerResult, compact_next_action},
    color::repo_color,
    format::{
        CheckOutcome, REFRESH_GLYPH, WORKTREE_GLYPH, comment_counts, format_age, format_repo_short,
        format_review_state, outcome, pad_to_width, truncate_middle,
    },
    github::rest::PR,
    github::types::Review,
    palette::Palette,
};

#[cfg(test)]
mod tests;

/// Render the PR list region. Renders the empty/loading placeholder, or a
/// column header followed by the grouped display rows: a header per group
/// (`── Me blocking `) followed by one PR row.
pub fn render(
    model: &Model,
    frame: &mut Frame<'_>,
    area: Rect,
    now: DateTime<Utc>,
    palette: &Palette,
) {
    let pr_list = &model.list;
    if pr_list.visible_is_empty() {
        // A filter that hid everything beats the fetch-state placeholders;
        // both checks are judged against the active tab's scope (a repo tab
        // shows its own repo's PRs and listing state, the All tab any repo).
        let scope = model.active_scope();
        let text = if pr_list.filter_hid_everything(scope.as_ref()) {
            "No matching PRs"
        } else if pr_list.is_loading(scope.as_ref()) {
            "Loading pull requests…"
        } else {
            "No open pull requests"
        };
        let placeholder = Paragraph::new(Line::from(text)).alignment(Alignment::Center);
        frame.render_widget(placeholder, area);
        return;
    }

    let [header_area, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let width = area.width;
    let prs = pr_list.prs();
    // Size columns to the visible PRs only, so an off-tab PR's wide number or
    // diff size can't widen this tab's columns.
    let visible: Vec<&PR> = pr_list.visible_pr_indices().map(|i| &prs[i]).collect();
    let table = pr_table(
        usize::from(width),
        should_show_repo_column(model),
        pr_num_col_width(&visible),
        size_col_width(&visible),
    );
    frame.render_widget(
        Paragraph::new(table.header(Style::default().add_modifier(Modifier::BOLD))),
        header_area,
    );

    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(row, selected)| match row {
            DisplayRow::Header(label) => header_line(label, model.list.grouping(), width, palette),
            DisplayRow::Pr(index) => {
                let pr = &prs[*index];
                row_line(pr, model, &table, now, selected, palette)
            }
        })
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, rows_area);
}

const PR_NUM_COL_MIN: usize = 7;
const SIZE_SIDE_COL_MIN: usize = 6;
const SIZE_COL_MIN: usize = SIZE_SIDE_COL_MIN * 2 + 1;

/// The Open PR List's columns, in display order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PrColumn {
    Indicator,
    Number,
    Repo,
    Title,
    Author,
    Size,
    Updated,
    Review,
    Action,
}

/// The one PR column declaration, fitted to `row_width`. Optional metadata is
/// admitted by rank — Updated, Author, Size, Review, Action — while the Title
/// keeps 30 columns, so shrinking hides Action first and Updated last.
fn pr_table(
    row_width: usize,
    show_repo: bool,
    pr_num_col: usize,
    size_col: usize,
) -> Table<PrColumn> {
    Columns::new()
        .column(Column::fixed(PrColumn::Indicator, "", 1))
        .column(Column::fixed(PrColumn::Number, "PR", pr_num_col))
        .column_if(show_repo, Column::fixed(PrColumn::Repo, "Repo", 14))
        .fill(PrColumn::Title, "Title", FillReserves::new(30, 30))
        .column(Column::fixed(PrColumn::Author, "Author", 14).optional(1))
        .column(Column::fixed(PrColumn::Size, "Size", size_col).optional(2))
        .column(Column::fixed(PrColumn::Updated, "Updated", 7).optional(0))
        .column(Column::fixed(PrColumn::Review, "Review", 18).optional(3))
        .column(Column::fixed(PrColumn::Action, "Action", 26).optional(4))
        .fit(row_width)
}

/// Whether the All tab shows the repo column. Keys off the tracked-repo count
/// (mirroring the TS `showRepo`) rather than the repo spread of the visible
/// PRs: a structural condition stays put while PRs stream in or a filter
/// narrows the list, so the columns never shift mid-read.
fn should_show_repo_column(model: &Model) -> bool {
    model.active_scope().is_none() && model.tracked_repos().len() > 1
}

/// Width of the `#<number>` column, sized to fit the widest visible PR number.
/// Floored at `PR_NUM_COL_MIN` so single-digit-PR repos still get a
/// readable two-column gap; widens uniformly once PR numbers cross 5 chars
/// (e.g. `#12345`) so the title column doesn't drift row-by-row.
fn pr_num_col_width(prs: &[&PR]) -> usize {
    let widest = prs
        .iter()
        .map(|pr| format!("#{}", pr.number).chars().count())
        .max()
        .unwrap_or(0);
    widest.max(PR_NUM_COL_MIN)
}

/// Width of the centred `+A/-D` size column. Each side reserves room for a sign
/// plus five digits, with the slash fixed in the middle. PRs over five digits
/// widen both sides uniformly so the slash remains vertically aligned.
fn size_col_width(prs: &[&PR]) -> usize {
    let widest_side = prs
        .iter()
        .flat_map(|pr| {
            [
                signed_size_width(pr.additions),
                signed_size_width(pr.deletions),
            ]
        })
        .max()
        .unwrap_or(0);
    (widest_side.max(SIZE_SIDE_COL_MIN) * 2 + 1).max(SIZE_COL_MIN)
}

fn signed_size_width(value: u64) -> usize {
    1 + value.to_string().len()
}

fn format_list_size(pr: &PR, width: usize) -> String {
    if !pr.review_status_loaded {
        return centered_ellipsis(width);
    }

    let additions = pr.additions;
    let deletions = pr.deletions;
    let left = format!("+{additions}");
    let right = format!("-{deletions}");
    let left_width = width.saturating_sub(1) / 2;
    let right_width = width.saturating_sub(1) - left_width;

    format!(
        "{}{left}/{right}{}",
        " ".repeat(left_width.saturating_sub(left.len())),
        " ".repeat(right_width.saturating_sub(right.len()))
    )
}

fn centered_ellipsis(width: usize) -> String {
    let padding = width.saturating_sub(1);
    let left = padding / 2;
    let right = padding - left;
    format!("{}…{}", " ".repeat(left), " ".repeat(right))
}

/// A group header row: `  ── <label> `, padded to the row width and bolded.
/// Visually distinct from PR rows (the leading rule and colour). A repo group's
/// header takes the repo's stable Repo Color so the boundaries between repos are
/// obvious; every other grouping keeps the accent colour (Smart-status tier
/// headers keep their accent rather than borrowing a tier colour, so tier
/// meaning stays a property of the action cell, not the header rule).
fn header_line(label: &str, grouping: Grouping, width: u16, palette: &Palette) -> Line<'static> {
    let text = pad_to_width(&format!("  ── {label} "), width as usize);
    // Under repo grouping the header label is the repo slug (`parse_pr` always
    // stamps a non-empty `owner/repo`), so it resolves to that repo's colour.
    let fg = match grouping {
        Grouping::Repo => repo_color(label),
        _ => palette.accent,
    };
    Line::from(Span::styled(
        text,
        Style::default().fg(fg).add_modifier(Modifier::BOLD),
    ))
}

/// One PR's display row: the fitted table asks for each present column's
/// content by ID, so the row can neither omit nor reorder a column.
fn row_line(
    pr: &PR,
    model: &Model,
    table: &Table<PrColumn>,
    now: DateTime<Utc>,
    selected: bool,
    palette: &Palette,
) -> Line<'static> {
    // The Selected Row brightens only the title to `selected_fg`; every other
    // cell keeps its semantic foreground over the `selected_bg` band the table
    // lays under the whole line (see ADR 0005).
    let title_style = if selected {
        Style::default().fg(palette.selected_fg)
    } else {
        Style::default()
    };
    table.row(selected.then_some(palette.selected_bg), |column, width| {
        let (text, style) = match column {
            PrColumn::Indicator => {
                let (glyph, style) = leading_glyph(pr, model, palette);
                (glyph.to_owned(), style)
            }
            PrColumn::Number => (
                format!("#{}", pr.number),
                Style::default()
                    .fg(palette.count)
                    .add_modifier(Modifier::BOLD),
            ),
            // The repo cell takes the repo's stable Repo Color, so a mixed
            // All-tab list groups visually by repo while scanning.
            PrColumn::Repo => (
                truncate_middle(format_repo_short(pr.repo_slug.as_str()), width),
                Style::default().fg(repo_color(pr.repo_slug.as_str())),
            ),
            PrColumn::Title => (pr.title.clone(), title_style),
            PrColumn::Author => (
                truncate_middle(&pr.author, width),
                Style::default().fg(palette.author),
            ),
            PrColumn::Size => (format_list_size(pr, width), Style::default()),
            PrColumn::Updated => (format_age(pr.updated_at, now), Style::default()),
            PrColumn::Review => review_cell(pr, model, palette),
            PrColumn::Action => action_cell(model.blockers.get(&pr.key()), palette),
        };
        vec![Span::styled(text, style)]
    })
}

/// The leading one-column glyph for a PR row, with its colour: the refresh
/// indicator while the PR's `r`/`R` refresh is in flight, the worktree glyph
/// when one is attached, else empty. The refresh indicator wins so an in-flight
/// refresh is visible even on a PR that also has a worktree.
fn leading_glyph(pr: &PR, model: &Model, palette: &Palette) -> (&'static str, Style) {
    if model.is_refreshing(pr) {
        (REFRESH_GLYPH, Style::default().fg(palette.accent))
    } else if model.worktree_for_pr(pr).is_some() {
        (WORKTREE_GLYPH, Style::default().fg(palette.accent))
    } else {
        ("", Style::default())
    }
}

fn review_cell(pr: &PR, model: &Model, palette: &Palette) -> (String, Style) {
    let checks = model.enrichment.checks_for(pr).unwrap_or(&[]);
    let has_failing_checks = checks
        .iter()
        .any(|check| outcome(check) == CheckOutcome::Failed);
    let reviews = model.enrichment.reviews.get(&pr.key()).map(Vec::as_slice);
    let thread_label = review_thread_label(pr, model);

    let mut parts = Vec::new();
    if pr.mergeable == "CONFLICTING" {
        parts.push("!".to_owned());
    }
    if has_failing_checks {
        parts.push("x".to_owned());
    }
    if let Some(label) = review_label(pr, reviews)
        && !label.is_empty()
    {
        parts.push(label);
    }
    let has_review_parts = !parts.is_empty();
    if let Some(text) = thread_label.text {
        parts.push(text);
    }
    if pr.is_draft {
        parts.push("draft".to_owned());
    }

    let color = if pr.mergeable == "CONFLICTING" || has_failing_checks {
        palette.failing
    } else if pr.is_draft || thread_label.unresolved_human {
        palette.pending
    } else if !has_review_parts && thread_label.has_text {
        palette.muted
    } else {
        palette.text
    };
    (parts.join(" "), Style::default().fg(color))
}

fn review_label(pr: &PR, reviews: Option<&[Review]>) -> Option<String> {
    if matches!(
        pr.review_decision.as_str(),
        "APPROVED" | "CHANGES_REQUESTED"
    ) {
        return Some(format_review_state(&pr.review_decision).to_owned());
    }

    if let Some(reviews) = reviews {
        if reviews.iter().any(|r| r.state == "CHANGES_REQUESTED") {
            return Some(format_review_state("CHANGES_REQUESTED").to_owned());
        }
        if reviews.iter().any(|r| r.state == "APPROVED") {
            return Some(format_review_state("APPROVED").to_owned());
        }
    }

    match pr.review_decision.as_str() {
        "" | "REVIEW_REQUIRED" | "COMMENTED" => None,
        other => Some(other.to_lowercase()),
    }
}

struct ReviewThreadLabel {
    text: Option<String>,
    has_text: bool,
    unresolved_human: bool,
}

fn review_thread_label(pr: &PR, model: &Model) -> ReviewThreadLabel {
    let Some(threads) = model.enrichment.review_threads.get(&pr.key()) else {
        return ReviewThreadLabel {
            text: Some("…".to_owned()),
            has_text: true,
            unresolved_human: false,
        };
    };
    let counts = comment_counts(threads, &model.config.bot_logins);
    let mut parts = Vec::new();
    if counts.unresolved_human > 0 {
        parts.push(format!("{}H", counts.unresolved_human));
    }
    if counts.unresolved_bot > 0 {
        parts.push(format!("{}B", counts.unresolved_bot));
    }
    let text = (!parts.is_empty()).then(|| parts.join(" "));
    ReviewThreadLabel {
        has_text: text.is_some(),
        text,
        unresolved_human: counts.unresolved_human > 0,
    }
}

fn action_cell(blocker: Option<&BlockerResult>, palette: &Palette) -> (String, Style) {
    let Some(blocker) = blocker else {
        return ("…".to_owned(), Style::default().fg(palette.muted));
    };
    (
        compact_next_action(blocker),
        Style::default().fg(palette.tier(blocker.tier)),
    )
}
