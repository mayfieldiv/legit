//! Column tables declared once and fitted to a row width. A surface lists its
//! columns in display order — fixed-width columns, optionally admitted by
//! rank, around exactly one Fill column — and fits them once per render. The
//! fitted [`Table`] then renders the header and every row from the same
//! present columns, so labels and data cannot drift and hidden columns leave
//! neither a cell nor a gap.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::row::{Cell, GAP, render_cells};

#[cfg(test)]
mod tests;

/// Title space the Fill column keeps from the two allocation phases: optional
/// columns are admitted only while `admission_reserve` columns remain for the
/// Fill, and fitted columns grow only from surplus above `growth_reserve`.
/// Neither is a floor — the Fill still shrinks to one column when required
/// columns alone overflow the row.
pub struct FillReserves {
    admission_reserve: usize,
    // TODO(#140): read by fitted-column growth once the ticket queue migrates.
    #[allow(dead_code)]
    growth_reserve: usize,
}

impl FillReserves {
    /// Panics unless `1 <= admission_reserve <= growth_reserve`.
    pub fn new(admission_reserve: usize, growth_reserve: usize) -> Self {
        assert!(
            admission_reserve >= 1,
            "fill admission reserve must be at least one column"
        );
        assert!(
            admission_reserve <= growth_reserve,
            "fill growth reserve ({growth_reserve}) must not be below its admission reserve ({admission_reserve})"
        );
        Self {
            admission_reserve,
            growth_reserve,
        }
    }
}

enum Admission {
    Required,
    Optional { rank: u8 },
}

/// One non-Fill column declaration: identity, header label, width, and
/// whether the fitter may drop it for space.
pub struct Column<Id> {
    id: Id,
    header: &'static str,
    width: usize,
    admission: Admission,
}

impl<Id> Column<Id> {
    /// A required column of exactly `width` terminal columns. Panics on a zero
    /// width; an absent column is declared by omitting it (`column_if`), not by
    /// a zero width.
    pub fn fixed(id: Id, header: &'static str, width: usize) -> Self {
        assert!(width >= 1, "fixed column {header:?} needs a positive width");
        Self {
            id,
            header,
            width,
            admission: Admission::Required,
        }
    }

    /// Let the fitter drop this column when the row is too narrow. Lower ranks
    /// are tried first; equal ranks fall back to declaration order.
    pub fn optional(self, admission_rank: u8) -> Self {
        Self {
            admission: Admission::Optional {
                rank: admission_rank,
            },
            ..self
        }
    }
}

/// Builder state before the Fill column is declared: only `fill` leads on.
pub struct BeforeFill;

/// Builder state after the Fill column is declared: only `fit` leads on.
pub struct AfterFill<Id> {
    fill_id: Id,
    fill_header: &'static str,
    reserves: FillReserves,
    /// Index in the display order where the Fill sits, counted over the
    /// declarations preceding it.
    fill_position: usize,
}

/// Ordered column declarations under construction. The type state makes
/// exactly one Fill structural: `fit` is unavailable until `fill` is called,
/// and `fill` is unavailable afterwards.
pub struct Columns<Id, State> {
    columns: Vec<Column<Id>>,
    state: State,
}

impl<Id> Default for Columns<Id, BeforeFill> {
    fn default() -> Self {
        Self::new()
    }
}

impl<Id> Columns<Id, BeforeFill> {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
            state: BeforeFill,
        }
    }

    /// Declare the one column that takes whatever width the others leave.
    pub fn fill(
        self,
        id: Id,
        header: &'static str,
        reserves: FillReserves,
    ) -> Columns<Id, AfterFill<Id>> {
        Columns {
            state: AfterFill {
                fill_id: id,
                fill_header: header,
                reserves,
                fill_position: self.columns.len(),
            },
            columns: self.columns,
        }
    }
}

impl<Id, State> Columns<Id, State> {
    pub fn column(mut self, column: Column<Id>) -> Self {
        self.columns.push(column);
        self
    }

    /// Declare `column` only when `enabled`; a disabled column is absent from
    /// the table structurally rather than dropped for space.
    pub fn column_if(self, enabled: bool, column: Column<Id>) -> Self {
        if enabled { self.column(column) } else { self }
    }
}

impl<Id: Copy + Eq> Columns<Id, AfterFill<Id>> {
    /// Fit the declaration to `row_width` terminal columns.
    ///
    /// Required columns are always present. Optional columns are tried by
    /// ascending rank (declaration order breaks ties) while the Fill keeps its
    /// admission reserve; each costs its width plus one gap, a failed attempt
    /// spends nothing and later candidates still get their turn. The Fill then
    /// takes what remains, never less than one column — so required columns
    /// wider than the row produce a line wider than the row, which the caller's
    /// widget clips.
    ///
    /// Panics when two declarations (including the Fill and unadmitted
    /// optionals) share an ID: a declaration is developer-authored, so a
    /// duplicate is a programming error, not a runtime condition.
    pub fn fit(self, row_width: usize) -> Table<Id> {
        let Columns { columns, state } = self;
        assert_unique_ids(&columns, state.fill_id);

        let mut present = vec![false; columns.len()];
        let mut used = 0;
        for (index, column) in columns.iter().enumerate() {
            if matches!(column.admission, Admission::Required) {
                present[index] = true;
                used += column.width + GAP;
            }
        }

        let mut candidates: Vec<(u8, usize)> = columns
            .iter()
            .enumerate()
            .filter_map(|(index, column)| match column.admission {
                Admission::Optional { rank } => Some((rank, index)),
                Admission::Required => None,
            })
            .collect();
        candidates.sort_by_key(|&(rank, index)| (rank, index));

        let mut budget = row_width
            .saturating_sub(used)
            .saturating_sub(state.reserves.admission_reserve);
        for (_, index) in candidates {
            let cost = columns[index].width + GAP;
            if budget >= cost {
                budget -= cost;
                present[index] = true;
                used += cost;
            }
        }

        let fill_width = row_width.saturating_sub(used).max(1);
        let fill = FittedColumn {
            id: state.fill_id,
            header: state.fill_header,
            width: fill_width,
        };

        let declared = columns.len();
        let mut fitted = Vec::with_capacity(declared + 1);
        for (index, column) in columns.into_iter().enumerate() {
            if index == state.fill_position {
                fitted.push(fill);
            }
            if present[index] {
                fitted.push(FittedColumn {
                    id: column.id,
                    header: column.header,
                    width: column.width,
                });
            }
        }
        if state.fill_position == declared {
            fitted.push(fill);
        }
        Table { columns: fitted }
    }
}

fn assert_unique_ids<Id: Copy + Eq>(columns: &[Column<Id>], fill_id: Id) {
    let ids: Vec<Id> = columns
        .iter()
        .map(|column| column.id)
        .chain(std::iter::once(fill_id))
        .collect();
    for (i, id) in ids.iter().enumerate() {
        assert!(
            !ids[..i].contains(id),
            "table declares a column ID twice (declaration index {i})"
        );
    }
}

#[derive(Clone, Copy)]
struct FittedColumn<Id> {
    id: Id,
    header: &'static str,
    width: usize,
}

/// A declaration fitted to one row width: only the present columns, in
/// display order, each with a positive width. Immutable; fit once per render
/// and reuse for the header and every row.
pub struct Table<Id> {
    columns: Vec<FittedColumn<Id>>,
}

impl<Id: Copy + Eq> Table<Id> {
    /// The width assigned to `id`, or `None` when the column is absent —
    /// omitted from the declaration or not admitted at this row width.
    // TODO(#140): the ticket queue's Type-presence query is the first
    // production caller; until it migrates only tests read fitted widths.
    #[allow(dead_code)]
    pub fn width(&self, id: Id) -> Option<usize> {
        self.columns
            .iter()
            .find(|column| column.id == id)
            .map(|column| column.width)
    }

    /// The header row: every present column's label in `style`.
    pub fn header(&self, style: Style) -> Line<'static> {
        let cells = self
            .columns
            .iter()
            .map(|column| Cell::text(column.header, column.width, style))
            .collect();
        render_cells(cells, None)
    }

    /// One data row. `cell` is called once per present column, in display
    /// order, with the column's ID and assigned width, and returns the styled
    /// spans for that cell; spans are fitted to the width sequentially, then
    /// padded. `fill` paints a background under the whole line.
    pub fn row(
        &self,
        fill: Option<Color>,
        mut cell: impl FnMut(Id, usize) -> Vec<Span<'static>>,
    ) -> Line<'static> {
        let cells = self
            .columns
            .iter()
            .map(|column| Cell {
                spans: cell(column.id, column.width),
                width: column.width,
            })
            .collect();
        render_cells(cells, fill)
    }
}
