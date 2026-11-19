//! Large text drawn with block characters.
//!
//! Glyphs come from a 5 pixel tall bitmap font and are drawn with the full
//! block and the upper/lower half blocks only, which every console font
//! (Linux VT and kmscon) provides. A cell holds two vertically stacked pixels
//! so that, with the usual 1:2 cell aspect ratio, pixels stay square.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Height of every glyph, in pixels.
pub const GLYPH_HEIGHT: usize = 5;
/// Blank pixel columns between two glyphs.
const GLYPH_SPACING: usize = 1;
/// Pixels stacked in a single terminal cell.
const PIXELS_PER_CELL: u16 = 2;
/// Largest scale used, so digits stand out without dominating large tiles.
pub const MAXIMUM_SCALE: u16 = 2;
const PIXEL_ON: u8 = b'#';
const FULL_BLOCK: char = '█';
const UPPER_HALF_BLOCK: char = '▀';
const LOWER_HALF_BLOCK: char = '▄';
const EMPTY_CELL: char = ' ';

/// Bitmap of a glyph, one string per pixel row where `#` is a lit pixel.
pub type Glyph = [&'static str; GLYPH_HEIGHT];

/// A run of text drawn with a single style.
#[derive(Debug, Clone, PartialEq)]
pub struct StyledText<'a> {
    /// Characters to draw.
    pub text: &'a str,
    /// Style applied to the lit pixels of those characters.
    pub style: Style,
}

/// One pixel column of the rendered text with the style of its glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PixelColumn {
    lit_rows: [bool; GLYPH_HEIGHT],
    style: Style,
}

/// Returns the bitmap for `character`, or `None` when the font lacks it.
///
/// The font covers what formatted values contain: digits, `.`, `-`, `%`,
/// `/`, `:`, `°`, the unit suffixes (`B`, `KiB`..`PiB`, `C`, `d`, `h`, `m`,
/// `s`) and `NaN`.
pub fn glyph(character: char) -> Option<Glyph> {
    let bitmap = match character {
        '0' => ["###", "#.#", "#.#", "#.#", "###"],
        '1' => [".#", "##", ".#", ".#", ".#"],
        '2' => ["###", "..#", "###", "#..", "###"],
        '3' => ["###", "..#", "###", "..#", "###"],
        '4' => ["#.#", "#.#", "###", "..#", "..#"],
        '5' => ["###", "#..", "###", "..#", "###"],
        '6' => ["###", "#..", "###", "#.#", "###"],
        '7' => ["###", "..#", "..#", "..#", "..#"],
        '8' => ["###", "#.#", "###", "#.#", "###"],
        '9' => ["###", "#.#", "###", "..#", "###"],
        '.' => [".", ".", ".", ".", "#"],
        ':' => [".", "#", ".", "#", "."],
        ' ' => ["..", "..", "..", "..", ".."],
        '-' => ["...", "...", "###", "...", "..."],
        '/' => ["..#", "..#", ".#.", "#..", "#.."],
        '%' => ["#.#", "..#", ".#.", "#..", "#.#"],
        '°' => ["##", "##", "..", "..", ".."],
        'B' => ["##.", "#.#", "##.", "#.#", "##."],
        'C' => ["###", "#..", "#..", "#..", "###"],
        'G' => ["###", "#..", "#.#", "#.#", "###"],
        'K' => ["#.#", "#.#", "##.", "#.#", "#.#"],
        'M' => ["#...#", "##.##", "#.#.#", "#...#", "#...#"],
        'N' => ["#..#", "##.#", "#.##", "#..#", "#..#"],
        'P' => ["###", "#.#", "###", "#..", "#.."],
        'T' => ["###", ".#.", ".#.", ".#.", ".#."],
        'a' => ["...", ".##", "#.#", "#.#", ".##"],
        'd' => ["..#", "..#", ".##", "#.#", ".##"],
        'h' => ["#..", "#..", "##.", "#.#", "#.#"],
        'i' => ["#", ".", "#", "#", "#"],
        'm' => [".....", "##.#.", "#.#.#", "#.#.#", "#.#.#"],
        's' => [".##", "#..", ".#.", "..#", "##."],
        _ => return None,
    };
    Some(bitmap)
}

/// Largest integer scale, up to [`MAXIMUM_SCALE`], at which text
/// `pixel_width` pixels wide fits in `area_width` by `area_height` cells, or
/// `None` when it does not fit at all.
///
/// At scale `n` a pixel is `n` cells wide and `n` half cells tall.
pub fn fitting_scale(pixel_width: usize, area_width: u16, area_height: u16) -> Option<u16> {
    if pixel_width == 0 {
        return None;
    }
    let width_scale = usize::from(area_width) / pixel_width;
    let height_scale = usize::from(area_height * PIXELS_PER_CELL) / GLYPH_HEIGHT;
    let scale = u16::try_from(width_scale.min(height_scale))
        .unwrap_or(u16::MAX)
        .min(MAXIMUM_SCALE);
    (scale > 0).then_some(scale)
}

/// Draws `segments` as large text at the biggest scale fitting the area.
///
/// Returns `None` when a character is not in the font or the area is too
/// small even at the smallest scale, so callers can fall back to plain text.
pub fn render_lines(
    segments: &[StyledText],
    area_width: u16,
    area_height: u16,
) -> Option<Vec<Line<'static>>> {
    let columns = pixel_columns(segments)?;
    let scale = fitting_scale(columns.len(), area_width, area_height)?;
    Some(render_at_scale(&columns, scale))
}

/// Number of cell rows used by large text drawn at `scale`.
pub fn rendered_height(scale: u16) -> u16 {
    let pixel_rows = u16::try_from(GLYPH_HEIGHT).unwrap_or(u16::MAX) * scale;
    pixel_rows.div_ceil(PIXELS_PER_CELL)
}

fn pixel_columns(segments: &[StyledText]) -> Option<Vec<PixelColumn>> {
    let mut columns = Vec::new();
    for segment in segments {
        for character in segment.text.chars() {
            if !columns.is_empty() {
                columns.extend(blank_columns(GLYPH_SPACING, segment.style));
            }
            columns.extend(glyph_columns(glyph(character)?, segment.style));
        }
    }
    Some(columns)
}

fn blank_columns(count: usize, style: Style) -> impl Iterator<Item = PixelColumn> {
    std::iter::repeat_n(
        PixelColumn {
            lit_rows: [false; GLYPH_HEIGHT],
            style,
        },
        count,
    )
}

fn glyph_columns(bitmap: Glyph, style: Style) -> impl Iterator<Item = PixelColumn> {
    let width = bitmap[0].len();
    (0..width).map(move |column_index| PixelColumn {
        lit_rows: std::array::from_fn(|row_index| {
            bitmap[row_index].as_bytes()[column_index] == PIXEL_ON
        }),
        style,
    })
}

fn render_at_scale(columns: &[PixelColumn], scale: u16) -> Vec<Line<'static>> {
    (0..rendered_height(scale))
        .map(|row_index| render_row(columns, scale, row_index))
        .collect()
}

fn render_row(columns: &[PixelColumn], scale: u16, row_index: u16) -> Line<'static> {
    let top_pixel_row = usize::from(row_index * PIXELS_PER_CELL / scale);
    let bottom_pixel_row = usize::from((row_index * PIXELS_PER_CELL + 1) / scale);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for column in columns {
        let cell = cell_character(
            is_lit(column, top_pixel_row),
            is_lit(column, bottom_pixel_row),
        );
        let text = cell.to_string().repeat(usize::from(scale));
        push_merged(&mut spans, text, column.style);
    }
    Line::from(spans)
}

fn is_lit(column: &PixelColumn, pixel_row: usize) -> bool {
    column.lit_rows.get(pixel_row).copied().unwrap_or(false)
}

fn cell_character(is_top_lit: bool, is_bottom_lit: bool) -> char {
    match (is_top_lit, is_bottom_lit) {
        (true, true) => FULL_BLOCK,
        (true, false) => UPPER_HALF_BLOCK,
        (false, true) => LOWER_HALF_BLOCK,
        (false, false) => EMPTY_CELL,
    }
}

fn push_merged(spans: &mut Vec<Span<'static>>, text: String, style: Style) {
    match spans.last_mut() {
        Some(last) if last.style == style => last.content.to_mut().push_str(&text),
        _ => spans.push(Span::styled(text, style)),
    }
}

#[cfg(test)]
mod tests {
    use polaris_core::unit::{Unit, format_value};
    use ratatui::style::Color;

    use super::*;

    const DONUT_STYLE: Style = Style::new().fg(Color::Yellow);
    const DUFF_STYLE: Style = Style::new().fg(Color::DarkGray);

    fn plain(text: &str) -> Vec<StyledText<'_>> {
        vec![StyledText {
            text,
            style: DONUT_STYLE,
        }]
    }

    /// Width in pixels of `segments`, `None` when a glyph is missing.
    fn pixel_width(segments: &[StyledText]) -> Option<usize> {
        pixel_columns(segments).map(|columns| columns.len())
    }

    fn line_texts(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn every_glyph_has_rows_of_equal_width() {
        let characters = "0123456789.:-/%° BCGKMNPTadhims";
        for character in characters.chars() {
            let bitmap = glyph(character).expect("character is in the font");
            assert!(
                bitmap.iter().all(|row| row.len() == bitmap[0].len()),
                "ragged glyph for {character:?}"
            );
        }
    }

    #[test]
    fn font_covers_every_formatted_unit() {
        let springfield_values = [
            format_value(742.0, Unit::Count, 0),
            format_value(-3.5, Unit::None, 1),
            format_value(42.4, Unit::Percent, 1),
            format_value(0.81, Unit::Ratio, 0),
            format_value(16.0 * 1024.0 * 1024.0 * 1024.0, Unit::Bytes, 1),
            format_value(1024.0_f64.powi(5) * 3.0, Unit::Bytes, 0),
            format_value(1024.0_f64.powi(2), Unit::Bytes, 0),
            format_value(1024.0_f64.powi(4), Unit::Bytes, 0),
            format_value(1024.0, Unit::Bytes, 0),
            format_value(65.0, Unit::Celsius, 0),
            format_value(3.0 * 86400.0, Unit::Seconds, 0),
            format_value(7200.0, Unit::Seconds, 0),
            format_value(90.0, Unit::Seconds, 0),
            format_value(12.0, Unit::Seconds, 0),
            format_value(f64::NAN, Unit::None, 0),
        ];
        for value in springfield_values {
            assert!(
                pixel_width(&plain(&value)).is_some(),
                "missing glyph in {value:?}"
            );
        }
    }

    #[test]
    fn pixel_width_adds_spacing_between_glyphs() {
        assert_eq!(pixel_width(&plain("3/5")), Some(11));
        assert_eq!(pixel_width(&plain("1.5")), Some(8));
    }

    #[test]
    fn pixel_width_rejects_unknown_characters() {
        assert_eq!(pixel_width(&plain("Homer asleep")), None);
    }

    #[test]
    fn fitting_scale_picks_the_largest_scale() {
        assert_eq!(fitting_scale(11, 14, 3), Some(1));
        assert_eq!(fitting_scale(11, 40, 5), Some(2));
    }

    #[test]
    fn fitting_scale_is_capped() {
        assert_eq!(fitting_scale(11, 400, 100), Some(MAXIMUM_SCALE));
    }

    #[test]
    fn fitting_scale_rejects_small_areas() {
        assert_eq!(fitting_scale(11, 10, 3), None);
        assert_eq!(fitting_scale(11, 14, 2), None);
        assert_eq!(fitting_scale(0, 14, 3), None);
    }

    #[test]
    fn rendered_height_rounds_up_to_whole_cells() {
        assert_eq!(rendered_height(1), 3);
        assert_eq!(rendered_height(2), 5);
        assert_eq!(rendered_height(3), 8);
    }

    #[test]
    fn renders_half_blocks_at_scale_one() {
        let lines = render_lines(&plain("42"), 7, 3).expect("fits");

        assert_eq!(line_texts(&lines), ["█ █ ▀▀█", "▀▀█ █▀▀", "  ▀ ▀▀▀"]);
    }

    #[test]
    fn renders_full_blocks_at_scale_two() {
        let lines = render_lines(&plain("7"), 6, 5).expect("fits");

        assert_eq!(
            line_texts(&lines),
            ["██████", "    ██", "    ██", "    ██", "    ██"]
        );
    }

    #[test]
    fn keeps_segment_styles() {
        let segments = [
            StyledText {
                text: "3",
                style: DONUT_STYLE,
            },
            StyledText {
                text: "/5",
                style: DUFF_STYLE,
            },
        ];
        let lines = render_lines(&segments, 11, 3).expect("fits");
        let styles: Vec<Style> = lines[0].spans.iter().map(|span| span.style).collect();

        assert_eq!(styles, [DONUT_STYLE, DUFF_STYLE]);
    }

    #[test]
    fn render_lines_returns_none_when_too_small() {
        assert_eq!(render_lines(&plain("742"), 10, 3), None);
    }
}
