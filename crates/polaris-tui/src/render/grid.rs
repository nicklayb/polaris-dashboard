//! Converts the weighted layout grid into terminal rectangles.

use polaris_core::config::LayoutConfig;
use polaris_core::widget::Area;
use ratatui::layout::{Constraint, Layout, Rect};

/// Column and row rectangles of the grid for a given screen area.
pub struct Cells {
    columns: Vec<Rect>,
    rows: Vec<Rect>,
}

impl Cells {
    /// Splits `area` following the relative weights of `layout`.
    pub fn split(area: Rect, layout: &LayoutConfig) -> Self {
        let weights_to_constraints = |weights: &[u16]| {
            weights
                .iter()
                .map(|weight| Constraint::Fill(*weight))
                .collect::<Vec<_>>()
        };
        Self {
            columns: Layout::horizontal(weights_to_constraints(&layout.columns))
                .split(area)
                .to_vec(),
            rows: Layout::vertical(weights_to_constraints(&layout.rows))
                .split(area)
                .to_vec(),
        }
    }

    /// Rectangle covered by a widget area, spans included.
    ///
    /// Areas are validated against the grid when the configuration loads.
    pub fn area_of(&self, area: &Area) -> Rect {
        let first_column = self.columns[area.column];
        let last_column = self.columns[area.column + area.column_span - 1];
        let first_row = self.rows[area.row];
        let last_row = self.rows[area.row + area.row_span - 1];
        Rect {
            x: first_column.x,
            y: first_row.y,
            width: last_column.right() - first_column.x,
            height: last_row.bottom() - first_row.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_of_covers_spanned_cells() {
        let layout = LayoutConfig {
            columns: vec![1, 1, 2],
            rows: vec![1, 3],
        };
        let cells = Cells::split(Rect::new(0, 0, 160, 24), &layout);

        let springfield_area = cells.area_of(&Area {
            column: 1,
            row: 0,
            column_span: 2,
            row_span: 2,
        });

        assert_eq!(springfield_area, Rect::new(40, 0, 120, 24));
    }

    #[test]
    fn area_of_single_cell() {
        let layout = LayoutConfig {
            columns: vec![1, 1],
            rows: vec![1, 1],
        };
        let cells = Cells::split(Rect::new(0, 0, 100, 20), &layout);

        assert_eq!(
            cells.area_of(&Area {
                column: 1,
                row: 1,
                column_span: 1,
                row_span: 1
            }),
            Rect::new(50, 10, 50, 10)
        );
    }
}
