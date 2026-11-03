//! Table with one row per key and one column per query.

use polaris_core::data::TableData;
use polaris_core::widget::TableWidget;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Cell, Row, Table};

use crate::palette::{self, MUTED};

/// Width weight of the key column relative to value columns.
const KEY_COLUMN_WEIGHT: u16 = 2;
const VALUE_COLUMN_WEIGHT: u16 = 1;

/// Draws the table, coloring each value by its thresholds.
pub fn draw(frame: &mut Frame, area: Rect, block: Block, table: &TableWidget, data: &TableData) {
    let header = Row::new(
        std::iter::once(table.key_header.as_str())
            .chain(table.columns.iter().map(|column| column.header.as_str())),
    )
    .style(Style::new().fg(MUTED).bold());
    let rows = data.rows.iter().map(|row| {
        let value_cells = table
            .columns
            .iter()
            .zip(&row.values)
            .map(|(column, value)| {
                let presented = column.present(value.as_ref());
                Cell::from(presented.text).fg(palette::level_color(presented.level))
            });
        Row::new(std::iter::once(Cell::from(row.key.clone()).bold()).chain(value_cells))
    });
    let widths = std::iter::once(Constraint::Fill(KEY_COLUMN_WEIGHT)).chain(
        table
            .columns
            .iter()
            .map(|_| Constraint::Fill(VALUE_COLUMN_WEIGHT)),
    );
    frame.render_widget(Table::new(rows, widths).header(header).block(block), area);
}
