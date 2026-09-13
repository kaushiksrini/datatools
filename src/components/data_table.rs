use crate::file::sample_data::ParquetSampleData;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::{Color, Position},
    style::Modifier,
    symbols::{border, line},
    text::Span,
    widgets::Widget,
};
use std::cmp::min;

use crate::file::Renderable;
use crate::file::utils::commas;

const NUM_SPACES_BETWEEN_COLUMNS: u16 = 2;
const NUM_SPACES_AFTER_LINE_NUMBER: u16 = 2;

/// Number of columns starting at `start` whose widths fit in `available_width`
/// (at least 1, so an over-wide column still shows, clipped).
fn columns_fitting(widths: &[u16], start: usize, available_width: u16) -> usize {
    let mut acc = 0u16;
    let mut count = 0usize;
    for &w in widths.iter().skip(start) {
        if count > 0 && acc.saturating_add(w) > available_width {
            break;
        }
        acc = acc.saturating_add(w);
        count += 1;
    }
    count.max(1)
}

/// Smallest scroll offset at which the trailing columns fill `available_width`
/// (i.e. the last column is flush right). Accumulates from the right so the end
/// is always reachable regardless of column widths.
fn max_scroll_from_widths(widths: &[u16], available_width: u16) -> usize {
    let mut acc = 0u16;
    let mut fit = 0usize;
    for &w in widths.iter().rev() {
        if fit > 0 && acc.saturating_add(w) > available_width {
            break;
        }
        acc = acc.saturating_add(w);
        fit += 1;
    }
    widths.len().saturating_sub(fit.max(1))
}

pub struct DataTable<'a> {
    pub data: &'a ParquetSampleData,
    pub title: String,
    pub title_color: Color,
    pub border_style: border::Set,
    pub horizontal_scroll: usize,
    pub vertical_scroll: usize,
    pub selected_row: Option<usize>,
    pub selected_color: Color,
    pub border_color: Color,
}

impl<'a> DataTable<'a> {
    pub fn new(data: &'a ParquetSampleData) -> Self {
        Self {
            data,
            title: format!("Data Preview ({} rows)", commas(data.total_rows as u64)),
            title_color: Color::Cyan,
            border_style: border::ROUNDED,
            horizontal_scroll: 0,
            vertical_scroll: 0,
            selected_row: None,
            selected_color: Color::Rgb(60, 60, 60),
            border_color: Color::DarkGray,
        }
    }

    pub fn with_title(mut self, title: String) -> Self {
        self.title = title;
        self
    }

    pub fn with_colors(mut self, title: Color, selected: Color) -> Self {
        self.title_color = title;
        self.selected_color = selected;
        self
    }

    pub fn with_border_style(mut self, border_style: border::Set) -> Self {
        self.border_style = border_style;
        self
    }

    pub fn with_horizontal_scroll(mut self, offset: usize) -> Self {
        self.horizontal_scroll = offset;
        self
    }

    pub fn with_vertical_scroll(mut self, offset: usize) -> Self {
        self.vertical_scroll = offset;
        self
    }

    pub fn with_selected_row(mut self, row: Option<usize>) -> Self {
        self.selected_row = row;
        self
    }

    pub fn scroll_left(&mut self) {
        if self.horizontal_scroll > 0 {
            self.horizontal_scroll -= 1;
        }
    }

    pub fn scroll_right(&mut self) {
        let max_scroll = self.get_max_scroll();
        if self.horizontal_scroll < max_scroll {
            self.horizontal_scroll += 1;
        }
    }

    pub fn get_max_scroll(&self) -> usize {
        // Calculate how many columns we can show at reasonable width
        let available_width = 120; // Assume reasonable terminal width
        let min_column_width = 12; // Minimum width for readability
        let max_visible_columns = available_width / min_column_width;

        // Total columns minus visible columns
        self.data.total_columns.saturating_sub(max_visible_columns)
    }

    /// Rendered width of every column (from the rows in view). Basis for both the
    /// scroll bound and how many columns are drawn, so they stay in agreement.
    fn all_column_widths(&self) -> Vec<u16> {
        let win = self.data.loaded();
        let start = self
            .vertical_scroll
            .saturating_sub(win.start)
            .min(win.rows.len());
        self.calculate_column_widths(&self.data.flattened_columns, &win.rows[start..])
    }

    /// Maximum horizontal scroll offset for a render area `area_width` wide,
    /// sized from actual column widths so the last column is always reachable.
    pub fn max_horizontal_scroll(&self, area_width: u16) -> usize {
        let max_row_num = self.data.total_rows.saturating_sub(self.vertical_scroll);
        let row_num_section_width =
            (format!("{max_row_num}").len().max(4) as u16) + 2 * NUM_SPACES_AFTER_LINE_NUMBER + 1;
        let available_width = area_width.saturating_sub(row_num_section_width);
        max_scroll_from_widths(&self.all_column_widths(), available_width)
    }

    fn calculate_column_widths(
        &self,
        headers: &[String],
        visible_rows: &[Vec<String>],
    ) -> Vec<u16> {
        let mut widths = Vec::new();

        for (col_idx, header) in headers.iter().enumerate() {
            let mut max_width = header.len();

            // Check content width for this column
            for row in visible_rows {
                if let Some(cell) = row.get(col_idx) {
                    max_width = max_width.max(cell.len());
                }
            }

            // Use minimum width of 8 and maximum of 25 for readability, add spacing
            widths.push((min(max_width.max(8), 25) as u16) + NUM_SPACES_BETWEEN_COLUMNS);
        }

        widths
    }

    fn render_header_separator(&self, buf: &mut Buffer, area: Rect, x_row_separator: u16, y: u16) {
        let border_style = ratatui::style::Style::default().fg(self.border_color);

        // Draw horizontal line
        for x in 0..area.width {
            if let Some(cell) = buf.cell_mut(Position::new(x, y - 1)) {
                cell.set_symbol(line::HORIZONTAL).set_style(border_style);
            }
        }

        // Intersection with row number separator
        if let Some(cell) = buf.cell_mut(Position::new(x_row_separator - 1, y - 1)) {
            cell.set_symbol(line::HORIZONTAL_DOWN)
                .set_style(border_style);
        }
    }

    fn render_row_numbers(&self, buf: &mut Buffer, area: Rect, rows: &[Vec<String>]) {
        let mut y = area.y;

        for (row_idx, _) in rows.iter().enumerate() {
            let actual_row_num = row_idx + self.vertical_scroll + 1;
            let is_selected = self
                .selected_row
                .is_some_and(|selected| row_idx + self.vertical_scroll == selected);

            let row_num_formatted = format!("{}", actual_row_num);
            let mut style: ratatui::prelude::Style =
                ratatui::style::Style::default().fg(Color::DarkGray);
            if is_selected {
                style = style
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED);
            }
            let span = Span::styled(row_num_formatted, style);
            buf.set_span(0, y, &span, area.width);
            y += 1;
            if y >= area.bottom() {
                break;
            }
        }
    }

    fn render_header(
        &self,
        buf: &mut Buffer,
        x_start: u16,
        y: u16,
        headers: &[String],
        column_widths: &[u16],
        max_width: u16,
    ) {
        let mut x_offset = x_start;

        for (header, &width) in headers.iter().zip(column_widths) {
            if x_offset >= max_width {
                break;
            }

            let effective_width = width.saturating_sub(NUM_SPACES_BETWEEN_COLUMNS);
            let truncated = if header.len() > effective_width as usize {
                format!(
                    "{}...",
                    &header[..effective_width.saturating_sub(3) as usize]
                )
            } else {
                header.clone()
            };

            let style = ratatui::style::Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD);
            let span = Span::styled(truncated, style);

            buf.set_span(x_offset, y, &span, width);
            x_offset += width;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_data_row(
        &self,
        buf: &mut Buffer,
        x_start: u16,
        y: u16,
        row_data: &[String],
        column_widths: &[u16],
        is_selected: bool,
        max_width: u16,
    ) {
        let mut x_offset = x_start;

        let style = if is_selected {
            ratatui::style::Style::default()
                .bg(self.selected_color)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            ratatui::style::Style::default()
        };

        for (cell_data, &width) in row_data.iter().zip(column_widths) {
            if x_offset >= max_width {
                break;
            }

            let effective_width = width.saturating_sub(NUM_SPACES_BETWEEN_COLUMNS);
            let truncated = if cell_data.chars().count() > effective_width as usize {
                let truncated_chars: String = cell_data
                    .chars()
                    .take(effective_width.saturating_sub(1) as usize)
                    .collect();
                format!("{}…", truncated_chars)
            } else {
                cell_data.clone()
            };

            // Pad with spaces to fill the column width
            let padded = format!("{:width$}", truncated, width = width as usize);
            let span = Span::styled(padded, style);

            buf.set_span(x_offset, y, &span, width);
            x_offset += width;
        }
    }

    fn render_row_number_separator(
        &self,
        buf: &mut Buffer,
        x_row_separator: u16,
        y_start: u16,
        height: u16,
    ) {
        let border_style = ratatui::style::Style::default().fg(self.border_color);

        // Draw vertical line after row numbers
        for y in y_start..(y_start + height) {
            if let Some(cell) = buf.cell_mut(Position::new(x_row_separator - 1, y)) {
                cell.set_symbol(line::VERTICAL).set_style(border_style);
            }
        }
    }
}

impl<'a> Widget for DataTable<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.area() == 0 {
            return;
        }

        let win = self.data.loaded();

        // Calculate row number section width
        let max_row_num = self.data.total_rows.saturating_sub(self.vertical_scroll);
        let max_row_num_length = format!("{}", max_row_num).len().max(4) as u16;
        let row_num_section_width = max_row_num_length + 2 * NUM_SPACES_AFTER_LINE_NUMBER + 1;
        let x_row_separator = max_row_num_length + NUM_SPACES_AFTER_LINE_NUMBER + 1;

        // Calculate available width for data columns
        let available_width = area.width.saturating_sub(row_num_section_width);

        // Clamp scroll to the actual end (last column flush right), then show as
        // many columns as truly fit from there. Sizing by real column widths
        // (not a flat estimate) keeps wide columns from pushing the last ones
        // off-screen / out of reach.
        let all_widths = self.all_column_widths();
        let max_scroll = max_scroll_from_widths(&all_widths, available_width);
        let horizontal_scroll = self.horizontal_scroll.min(max_scroll);
        let max_visible_columns = columns_fitting(&all_widths, horizontal_scroll, available_width);

        // Get visible columns
        let visible_headers: Vec<String> = self
            .data
            .flattened_columns
            .iter()
            .skip(horizontal_scroll)
            .take(max_visible_columns)
            .cloned()
            .collect();

        // Get visible data for each row (apply vertical scroll). Rows live in a
        // bounded window; index into it relative to the window start.
        let row_skip = self.vertical_scroll.saturating_sub(win.start);
        let visible_rows: Vec<Vec<String>> = win
            .rows
            .iter()
            .skip(row_skip)
            .map(|row| {
                row.iter()
                    .skip(horizontal_scroll)
                    .take(max_visible_columns)
                    .cloned()
                    .collect()
            })
            .collect();

        // Calculate column widths
        let column_widths = self.calculate_column_widths(&visible_headers, &visible_rows);

        // Header area: 2 lines (header text + separator)
        let header_height = 2;
        let y_header = area.y;
        let y_first_record = area.y + header_height;

        // Row area: including row numbers and row content
        let rows_area = Rect::new(
            area.x,
            y_first_record,
            area.width,
            area.height.saturating_sub(header_height),
        );

        // Render row numbers
        self.render_row_numbers(buf, rows_area, &visible_rows);

        // Render header
        self.render_header(
            buf,
            row_num_section_width,
            y_header,
            &visible_headers,
            &column_widths,
            area.width,
        );

        // Render header separator (horizontal line below headers)
        self.render_header_separator(buf, area, x_row_separator, y_first_record);

        // Render data rows
        for (y_offset, (row_idx, row_data)) in
            (y_first_record..).zip(visible_rows.iter().enumerate())
        {
            if y_offset >= rows_area.bottom() {
                break;
            }
            let actual_row_num = row_idx + self.vertical_scroll;
            let is_selected = self
                .selected_row
                .is_some_and(|selected| actual_row_num == selected);

            self.render_data_row(
                buf,
                row_num_section_width,
                y_offset,
                row_data,
                &column_widths,
                is_selected,
                area.width,
            );
        }

        // Render vertical separator after row numbers
        self.render_row_number_separator(buf, x_row_separator, y_first_record, rows_area.height);
    }
}

impl Renderable for ParquetSampleData {
    fn render_content(&self, area: Rect, buf: &mut Buffer) {
        let table_component = DataTable::new(self);
        table_component.render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_columns_fitting_counts_what_fits() {
        let widths = [10, 10, 10, 10];
        // 10 + 10 = 20 <= 25; a third column would overflow.
        assert_eq!(columns_fitting(&widths, 0, 25), 2);
        // Exact fit.
        assert_eq!(columns_fitting(&widths, 0, 20), 2);
        // Counting from a non-zero offset.
        assert_eq!(columns_fitting(&widths, 2, 25), 2);
    }

    #[test]
    fn test_columns_fitting_shows_at_least_one() {
        // A single column wider than the whole area is still shown (clipped),
        // never zero columns.
        assert_eq!(columns_fitting(&[40], 0, 10), 1);
        assert_eq!(columns_fitting(&[40, 40], 0, 10), 1);
    }

    #[test]
    fn test_max_scroll_is_zero_when_all_columns_fit() {
        assert_eq!(max_scroll_from_widths(&[10, 10, 10], 100), 0);
    }

    #[test]
    fn test_max_scroll_reaches_the_last_column() {
        // Five 20-wide columns in 45 cells: two fit, so the last column becomes
        // flush-right at offset 3. A flat "width / 12" estimate capped this at 2
        // and left the final columns unreachable.
        let widths = [20, 20, 20, 20, 20];
        let max = max_scroll_from_widths(&widths, 45);
        assert_eq!(max, 3);
        // At the max offset the trailing columns (including the last) all fit.
        assert_eq!(max + columns_fitting(&widths, max, 45), widths.len());
    }

    #[test]
    fn test_max_scroll_with_a_wide_trailing_column() {
        // Only the last (over-wide) column fits from the right.
        assert_eq!(max_scroll_from_widths(&[10, 10, 50], 30), 2);
    }
}
