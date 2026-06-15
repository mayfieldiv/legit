use chrono::{DateTime, Utc};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    app::grouping::DisplayRow,
    app::model::Model,
    blocker::{BlockerResult, Tier, compact_next_action},
    format::{
        CheckOutcome, comment_counts, format_age, format_repo_short, format_review_state, outcome,
        pad_to_width, truncate, truncate_middle,
    },
    github::rest::PR,
    github::types::Review,
};

#[cfg(test)]
mod tests;

/// Render the PR list region. Renders the empty/loading placeholder, or a
/// column header followed by the grouped display rows: a header per group
/// (`── Me blocking `) followed by one PR row.
pub fn render(model: &Model, frame: &mut Frame<'_>, area: Rect, now: DateTime<Utc>) {
    let pr_list = &model.list;
    if pr_list.visible_is_empty() {
        // A filter that hid everything beats the fetch-state placeholders;
        // both checks are judged against the active tab's scope (a repo tab
        // shows its own repo's PRs and listing state, the All tab any repo).
        let scope = model.active_scope();
        let text = if pr_list.filter_hid_everything(scope.as_deref()) {
            "No matching PRs"
        } else if pr_list.is_loading(scope.as_deref()) {
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
    let pr_num_col = pr_num_col_width(&visible);
    let show_repo = should_show_repo_column(model);
    let size_col = size_col_width(&visible);
    let layout = RowLayout {
        width: usize::from(width),
        pr_num_col,
        size_col,
        show_repo,
        visible: compute_visible_columns(usize::from(width), show_repo, pr_num_col, size_col),
    };
    frame.render_widget(Paragraph::new(header_row_line(&layout)), header_area);

    let lines: Vec<Line<'_>> = pr_list
        .visible_rows()
        .map(|(row, selected)| match row {
            DisplayRow::Header(label) => header_line(label, width),
            DisplayRow::Pr(index) => {
                let pr = &prs[*index];
                row_line(pr, model, &layout, now, selected)
            }
        })
        .collect();
    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, rows_area);
}

const PR_NUM_COL_MIN: usize = 7;
const TITLE_MIN: usize = 30;
const AUTHOR_COL: usize = 14;
const REPO_COL: usize = 14;
const SIZE_SIDE_COL_MIN: usize = 6;
const SIZE_COL_MIN: usize = SIZE_SIDE_COL_MIN * 2 + 1;
const AGE_COL: usize = 6;
const REVIEW_COL: usize = 24;
const ACTION_COL: usize = 20;
const GAP: usize = 1;

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

/// A group header row: `── <label> ` in the accent colour, padded to the row
/// width. Visually distinct from PR rows (the leading rule and colour).
fn header_line(label: &str, width: u16) -> Line<'static> {
    let text = pad_to_width(&format!("── {label} "), width as usize);
    Line::from(Span::styled(
        text,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Per-render layout shared by every PR row: the row width and the column
/// sizing derived from the visible PRs (widest number/size, repo column on a
/// multi-repo All tab).
struct RowLayout {
    width: usize,
    pr_num_col: usize,
    size_col: usize,
    show_repo: bool,
    visible: VisibleColumns,
}

#[derive(Clone, Copy)]
struct VisibleColumns {
    author: bool,
    size: bool,
    age: bool,
    review: bool,
    action: bool,
}

/// Compute optional list-column visibility from the available list width.
/// Columns are added from most to least important, which means shrinking hides
/// them in the TS priority order: action -> review -> size -> author -> age.
fn compute_visible_columns(
    width: usize,
    show_repo: bool,
    pr_num_col: usize,
    size_col: usize,
) -> VisibleColumns {
    let base = pr_num_col + GAP + TITLE_MIN + usize::from(show_repo) * (REPO_COL + GAP);
    let mut budget = width.saturating_sub(base);
    let mut columns = VisibleColumns {
        age: false,
        author: false,
        size: false,
        review: false,
        action: false,
    };

    if reserve_visible_column(&mut budget, AGE_COL) {
        columns.age = true;
    }
    if reserve_visible_column(&mut budget, AUTHOR_COL) {
        columns.author = true;
    }
    if reserve_visible_column(&mut budget, size_col) {
        columns.size = true;
    }
    if reserve_visible_column(&mut budget, REVIEW_COL) {
        columns.review = true;
    }
    if reserve_visible_column(&mut budget, ACTION_COL) {
        columns.action = true;
    }

    columns
}

fn reserve_visible_column(budget: &mut usize, column_width: usize) -> bool {
    let cost = column_width + GAP;
    if *budget < cost {
        return false;
    }

    *budget -= cost;
    true
}

#[derive(Clone)]
struct Cell {
    text: String,
    width: usize,
    style: Style,
}

/// Column header row. Built from the same layout as PR rows so labels and data
/// cannot drift apart.
fn header_row_line(layout: &RowLayout) -> Line<'static> {
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let mut cells = base_cells("PR", layout, bold);
    if layout.show_repo {
        cells.push(Cell {
            text: "Repo".to_owned(),
            width: REPO_COL,
            style: bold,
        });
    }
    let title_slot = cells.len();
    if layout.visible.author {
        cells.push(Cell {
            text: "Author".to_owned(),
            width: AUTHOR_COL,
            style: bold,
        });
    }
    if layout.visible.size {
        cells.push(Cell {
            text: "Size".to_owned(),
            width: layout.size_col,
            style: bold,
        });
    }
    if layout.visible.age {
        cells.push(Cell {
            text: "Age".to_owned(),
            width: AGE_COL,
            style: bold,
        });
    }
    if layout.visible.review {
        cells.push(Cell {
            text: "Review".to_owned(),
            width: REVIEW_COL,
            style: bold,
        });
    }
    if layout.visible.action {
        cells.push(Cell {
            text: "Action".to_owned(),
            width: ACTION_COL,
            style: bold,
        });
    }
    insert_title_cell(&mut cells, title_slot, layout, "Title".to_owned(), bold);
    render_cells(cells, false)
}

/// One PR's display row. The fixed-width cells are built as data so the
/// leftover-title-width math and the rendered spans derive from the same list.
fn row_line(
    pr: &PR,
    model: &Model,
    layout: &RowLayout,
    now: DateTime<Utc>,
    selected: bool,
) -> Line<'static> {
    let mut cells = base_cells(
        &format!("#{}", pr.number),
        layout,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    if layout.show_repo {
        // Magenta is this port's mapping for the TS selfHighlight colour the
        // repo cell uses (the same mapping as the me-blocking tier).
        let repo = truncate_middle(format_repo_short(&pr.repo_slug), REPO_COL);
        cells.push(Cell {
            text: repo,
            width: REPO_COL,
            style: Style::default().fg(Color::Magenta),
        });
    }
    let title_slot = cells.len();
    if layout.visible.author {
        cells.push(Cell {
            text: truncate_middle(&pr.author, AUTHOR_COL),
            width: AUTHOR_COL,
            style: Style::default().fg(Color::Green),
        });
    }
    if layout.visible.size {
        cells.push(Cell {
            text: format_list_size(pr, layout.size_col),
            width: layout.size_col,
            style: Style::default(),
        });
    }
    if layout.visible.age {
        cells.push(Cell {
            text: format_age(pr.created_at, now),
            width: AGE_COL,
            style: Style::default(),
        });
    }
    if layout.visible.review {
        let (text, style) = review_cell(pr, model);
        cells.push(Cell {
            text,
            width: REVIEW_COL,
            style,
        });
    }
    if layout.visible.action {
        let (text, style) = action_cell(model.blockers.get(&pr.key()));
        cells.push(Cell {
            text,
            width: ACTION_COL,
            style,
        });
    }

    insert_title_cell(
        &mut cells,
        title_slot,
        layout,
        pr.title.clone(),
        Style::default(),
    );
    render_cells(cells, selected)
}

fn base_cells(text: &str, layout: &RowLayout, style: Style) -> Vec<Cell> {
    vec![Cell {
        text: text.to_owned(),
        width: layout.pr_num_col,
        style,
    }]
}

fn insert_title_cell(
    cells: &mut Vec<Cell>,
    title_slot: usize,
    layout: &RowLayout,
    text: String,
    style: Style,
) {
    let fixed: usize = cells.iter().map(|cell| cell.width).sum::<usize>() + cells.len() * GAP;
    let title_col = layout.width.saturating_sub(fixed).max(1);
    cells.insert(
        title_slot,
        Cell {
            text,
            width: title_col,
            style,
        },
    );
}

fn render_cells(cells: Vec<Cell>, selected: bool) -> Line<'static> {
    let mut spans = Vec::with_capacity(cells.len() * 2 - 1);
    for (i, cell) in cells.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(GAP)));
        }
        let text = truncate(&cell.text, cell.width);
        spans.push(Span::styled(pad_to_width(&text, cell.width), cell.style));
    }

    let line = Line::from(spans);
    if selected {
        line.style(Style::default().add_modifier(Modifier::REVERSED))
    } else {
        line
    }
}

fn review_cell(pr: &PR, model: &Model) -> (String, Style) {
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
        Color::Red
    } else if pr.is_draft || thread_label.unresolved_human {
        Color::Yellow
    } else if !has_review_parts && thread_label.has_text {
        Color::Gray
    } else {
        Color::Reset
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

fn action_cell(blocker: Option<&BlockerResult>) -> (String, Style) {
    let Some(blocker) = blocker else {
        return ("…".to_owned(), Style::default().fg(Color::Gray));
    };
    let color = match blocker.tier {
        Tier::MeBlocking => Color::Magenta,
        Tier::WaitingOnAuthor => Color::Yellow,
        Tier::NeedsReview => Color::Gray,
    };
    (compact_next_action(blocker), Style::default().fg(color))
}
