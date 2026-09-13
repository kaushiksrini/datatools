use crate::file::utils::human_readable_bytes;
use crate::file::{
    row_groups::{RowGroupColumnMetadata, RowGroupPageInfo},
    utils::commas,
};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Position, Rect},
    prelude::Color,
    style::Stylize,
    text::Line,
    widgets::{Block, Borders, Cell, Row, Table, Widget},
};

/// Component to display column-level metadata for a selected row group
pub struct RowGroupColumnMetadataComponent<'a> {
    column_metadata: &'a RowGroupColumnMetadata,
    pages: &'a RowGroupPageInfo,
}

impl<'a> RowGroupColumnMetadataComponent<'a> {
    pub fn new(column_metadata: &'a RowGroupColumnMetadata, pages: &'a RowGroupPageInfo) -> Self {
        Self {
            column_metadata,
            pages,
        }
    }
}

impl<'a> Widget for RowGroupColumnMetadataComponent<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let title = vec![
            " Column: ".into(),
            self.column_metadata.column_path.clone().yellow().bold(),
            " ".into(),
        ];

        let block = Block::bordered()
            .title(Line::from(title).centered())
            .borders(Borders::TOP)
            .border_style(ratatui::style::Style::default().fg(Color::Blue));

        let inner_area = block.inner(area);
        block.render(area, buf);

        let [features_area, contents_area] = Layout::vertical([
            Constraint::Length(3), // Feature indicators
            Constraint::Fill(1),   // other area
        ])
        .areas(inner_area);

        let [md_stats_area, page_area] = Layout::horizontal([
            Constraint::Fill(3), // Metadata table
            Constraint::Fill(5), // Pages table
        ])
        .areas(contents_area);

        // Render pages table
        self.render_pages_table(page_area, buf);

        // Split into three sections: feature indicators, stats table, and statistics
        let mut constraints = vec![
            Constraint::Length(7), // Metadata table
        ];

        // Add constraint for statistics table if statistics exist
        if self.column_metadata.statistics.is_some() {
            constraints.push(Constraint::Length(6)); // Statistics table
        }

        let vertical_areas = Layout::vertical(constraints).split(md_stats_area);

        self.render_feature_indicators(features_area, buf);
        self.render_metadata_table(vertical_areas[0], buf);

        // Render statistics table if available
        if self.column_metadata.statistics.is_some() {
            self.render_statistics_table(vertical_areas[1], buf);
        }
    }
}

impl<'a> RowGroupColumnMetadataComponent<'a> {
    fn render_metadata_table(&self, area: Rect, buf: &mut Buffer) {
        // Calculate compression ratio
        let compression_ratio = if self.column_metadata.total_compressed_size > 0 {
            format!(
                "{:.2}x",
                self.column_metadata.total_uncompressed_size as f64
                    / self.column_metadata.total_compressed_size as f64
            )
        } else {
            "N/A".to_string()
        };

        let file_offset = match self.column_metadata.file_offset {
            Some(offset) => Cell::from(commas(offset)).fg(Color::White),
            None => Cell::from("N/A").fg(Color::DarkGray),
        };

        let kv_pairs = vec![
            ("File Offset (B)", file_offset),
            (
                "Compressed Size",
                Cell::from(human_readable_bytes(
                    self.column_metadata.total_compressed_size as u64,
                ))
                .fg(Color::White),
            ),
            (
                "Uncompressed Size",
                Cell::from(human_readable_bytes(
                    self.column_metadata.total_uncompressed_size as u64,
                ))
                .fg(Color::White),
            ),
            (
                "Compression Ratio",
                Cell::from(compression_ratio).fg(Color::White),
            ),
            (
                "Compression Type",
                Cell::from(self.column_metadata.compression_type.clone()).fg(Color::White),
            ),
        ];

        let rows: Vec<Row> = kv_pairs
            .into_iter()
            .map(|(k, v)| Row::new(vec![Cell::from(k).bold().fg(Color::Cyan), v]))
            .collect();

        let table = Table::new(rows, vec![Constraint::Length(18), Constraint::Fill(1)]).block(
            Block::bordered()
                .title("Metadata")
                .border_style(ratatui::style::Style::default().fg(Color::Blue)),
        );

        table.render(area, buf);
    }

    fn render_feature_indicators(&self, area: Rect, buf: &mut Buffer) {
        // Create 1x4 horizontal grid for feature indicators
        let horizontal_areas = Layout::horizontal([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

        self.render_indicator_box(
            "Statistics",
            self.column_metadata.has_stats.has_stats,
            horizontal_areas[0],
            buf,
        );
        self.render_indicator_box(
            "Dict Page",
            self.column_metadata.has_stats.has_dictionary_page,
            horizontal_areas[1],
            buf,
        );
        self.render_indicator_box(
            "Bloom Filter",
            self.column_metadata.has_stats.has_bloom_filter,
            horizontal_areas[2],
            buf,
        );
        self.render_indicator_box(
            "Page Stats",
            self.column_metadata.has_stats.has_page_encoding_stats,
            horizontal_areas[3],
            buf,
        );
    }

    fn render_indicator_box(&self, title: &str, has_feature: bool, area: Rect, buf: &mut Buffer) {
        let (symbol, color) = if has_feature {
            ("✓", Color::Green)
        } else {
            ("✗", Color::Red)
        };

        let block = Block::bordered()
            .title(title)
            .border_style(ratatui::style::Style::default().fg(color))
            .title_style(ratatui::style::Style::default().fg(color).bold());

        let inner = block.inner(area);
        block.render(area, buf);

        // Center the symbol in the block
        if inner.width > 0 && inner.height > 0 {
            let symbol_x = inner.x + inner.width / 2;
            let symbol_y = inner.y + inner.height / 2;

            if symbol_x < inner.x + inner.width
                && symbol_y < inner.y + inner.height
                && let Some(cell) = buf.cell_mut(Position::new(symbol_x, symbol_y))
            {
                cell.set_symbol(symbol)
                    .set_style(ratatui::style::Style::default().fg(color).bold());
            }
        }
    }

    fn render_statistics_table(&self, area: Rect, buf: &mut Buffer) {
        if let Some(ref stats) = self.column_metadata.statistics {
            let null_count_str = stats
                .null_count
                .map(|c| c.to_string())
                .unwrap_or_else(|| "N/A".to_string());

            let distinct_count = match stats.distinct_count {
                Some(count) => Cell::from(count.to_string()).fg(Color::White),
                None => Cell::from("N/A").fg(Color::DarkGray),
            };

            let stat_pairs = vec![
                (
                    "Min",
                    Cell::from(stats.min.as_deref().unwrap_or("N/A").to_string()).fg(Color::White),
                ),
                (
                    "Max",
                    Cell::from(stats.max.as_deref().unwrap_or("N/A").to_string()).fg(Color::White),
                ),
                ("Null Count", Cell::from(null_count_str).fg(Color::White)),
                ("Distinct Count", distinct_count),
            ];

            let rows: Vec<Row> = stat_pairs
                .into_iter()
                .map(|(k, v)| Row::new(vec![Cell::from(k).bold().fg(Color::Magenta), v]))
                .collect();

            let table = Table::new(rows, vec![Constraint::Length(18), Constraint::Fill(1)]).block(
                Block::bordered()
                    .title("Statistics")
                    .border_style(ratatui::style::Style::default().fg(Color::Magenta)),
            );

            table.render(area, buf);
        }
    }

    fn render_pages_table(&self, area: Rect, buf: &mut Buffer) {
        use crate::file::utils::human_readable_bytes;

        // Create header
        let header = Row::new(vec![
            Cell::from("#").bold().fg(Color::Yellow),
            Cell::from("Page Type").bold().fg(Color::Yellow),
            Cell::from("Size").bold().fg(Color::Yellow),
            Cell::from("Rows").bold().fg(Color::Yellow),
            Cell::from("Encoding").bold().fg(Color::Yellow),
        ]);

        // Create rows from page info
        let rows: Vec<Row> = self
            .pages
            .page_infos
            .iter()
            .enumerate()
            .map(|(idx, page)| {
                Row::new(vec![
                    Cell::from((idx + 1).to_string()).fg(Color::White),
                    Cell::from(page.page_type.clone()).fg(Color::Cyan),
                    Cell::from(human_readable_bytes(page.size as u64)).fg(Color::White),
                    Cell::from(commas(page.rows as u64)).fg(Color::White),
                    Cell::from(page.encoding.clone()).fg(Color::Green),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            vec![
                Constraint::Max(3),  // Page Number
                Constraint::Fill(3), // Page Type
                Constraint::Fill(3), // Size
                Constraint::Fill(2), // Rows
                Constraint::Fill(3), // Encoding
            ],
        )
        .header(header)
        .block(
            Block::bordered()
                .title("Pages")
                .border_style(ratatui::style::Style::default().fg(Color::DarkGray)),
        );

        table.render(area, buf);
    }
}
