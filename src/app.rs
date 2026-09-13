use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use std::io;

use crate::file::parquet_ctx::ParquetCtx;
use crate::tabs::TabManager;

pub struct AppRenderView<'a> {
    pub title: &'a str,
    pub parquet_ctx: &'a ParquetCtx,
    file_name: &'a str,
    tabs: &'a TabManager,
    pub state: &'a AppState,
}

impl<'a> AppRenderView<'a> {
    fn from_app(app: &'a App) -> Self {
        Self {
            title: "parqeye",
            parquet_ctx: app.parquet_ctx,
            file_name: &app.file_name,
            tabs: &app.tabs,
            state: &app.state,
        }
    }

    pub fn tabs(&self) -> &TabManager {
        self.tabs
    }

    pub fn file_name(&self) -> &str {
        self.file_name
    }

    pub fn state(&self) -> &AppState {
        self.state
    }
}

pub struct App<'a> {
    pub parquet_ctx: &'a ParquetCtx,
    pub file_name: String,
    pub exit: bool,
    pub tabs: TabManager,
    pub state: AppState,
}

pub struct AppState {
    horizontal_offset: usize,
    vertical_offset: usize,
    tree_scroll_offset: usize,
    data_vertical_scroll: usize,
    visible_data_rows: usize,
    // Upper bound for `horizontal_offset`, recomputed each frame from the
    // on-screen column count. Prevents scrolling past the last visible column.
    max_horizontal_offset: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn new() -> Self {
        Self {
            horizontal_offset: 0,
            vertical_offset: 0,
            tree_scroll_offset: 0,
            data_vertical_scroll: 0,
            visible_data_rows: 20, // Default fallback
            max_horizontal_offset: usize::MAX,
        }
    }

    pub fn reset(&mut self) {
        self.horizontal_offset = 0;
        self.vertical_offset = 0;
        self.tree_scroll_offset = 0;
        self.data_vertical_scroll = 0;
    }

    pub fn horizontal_offset(&self) -> usize {
        self.horizontal_offset
    }

    pub fn vertical_offset(&self) -> usize {
        self.vertical_offset
    }

    pub fn down(&mut self) {
        self.vertical_offset += 1;
    }

    pub fn up(&mut self) {
        self.vertical_offset = self.vertical_offset.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.horizontal_offset = (self.horizontal_offset + 1).min(self.max_horizontal_offset);
    }

    pub fn left(&mut self) {
        self.horizontal_offset = self.horizontal_offset.saturating_sub(1);
    }

    /// Set the upper bound for horizontal scrolling and clamp the current offset
    /// to it (handles overshoot from a previous frame and terminal resizes).
    pub fn set_max_horizontal_offset(&mut self, max: usize) {
        self.max_horizontal_offset = max;
        self.horizontal_offset = self.horizontal_offset.min(max);
    }

    pub fn tree_scroll_offset(&self) -> usize {
        self.tree_scroll_offset
    }

    pub fn tree_scroll_up(&mut self) {
        self.tree_scroll_offset = self.tree_scroll_offset.saturating_sub(1);
    }

    pub fn tree_scroll_down(&mut self) {
        self.tree_scroll_offset += 1;
    }

    pub fn data_vertical_scroll(&self) -> usize {
        self.data_vertical_scroll
    }

    pub fn set_data_vertical_scroll(&mut self, scroll: usize) {
        self.data_vertical_scroll = scroll;
    }

    pub fn visible_data_rows(&self) -> usize {
        self.visible_data_rows
    }

    pub fn set_visible_data_rows(&mut self, rows: usize) {
        self.visible_data_rows = rows;
    }

    pub fn page_up(&mut self, visible_rows: usize, max_rows: usize) {
        // Move selection up by visible_rows
        self.vertical_offset = self.vertical_offset.saturating_sub(visible_rows);
        // Adjust scroll to keep selection visible
        self.adjust_scroll_to_selection(visible_rows, max_rows);
    }

    pub fn page_down(&mut self, visible_rows: usize, max_rows: usize) {
        // Move selection down by visible_rows, clamped to max_rows - 1
        self.vertical_offset =
            (self.vertical_offset + visible_rows).min(max_rows.saturating_sub(1));
        // Adjust scroll to keep selection visible
        self.adjust_scroll_to_selection(visible_rows, max_rows);
    }

    /// Jump the selection (and viewport) to the first row.
    pub fn jump_to_top(&mut self) {
        self.vertical_offset = 0;
        self.data_vertical_scroll = 0;
    }

    /// Jump the selection (and viewport) to the last row.
    pub fn jump_to_bottom(&mut self, visible_rows: usize, max_rows: usize) {
        self.vertical_offset = max_rows.saturating_sub(1);
        self.adjust_scroll_to_selection(visible_rows, max_rows);
    }

    pub fn adjust_scroll_to_selection(&mut self, visible_rows: usize, max_rows: usize) {
        // Ensure selected row is visible in viewport
        if self.vertical_offset < self.data_vertical_scroll {
            // Selection is above viewport, scroll up
            self.data_vertical_scroll = self.vertical_offset;
        } else if self.vertical_offset >= self.data_vertical_scroll + visible_rows {
            // Selection is below viewport, scroll down
            self.data_vertical_scroll = self.vertical_offset.saturating_sub(visible_rows - 1);
        }

        // Clamp scroll to valid range
        let max_scroll = max_rows.saturating_sub(visible_rows);
        self.data_vertical_scroll = self.data_vertical_scroll.min(max_scroll);
    }
}

impl<'a> App<'a> {
    pub fn new(file_info: &'a ParquetCtx) -> Self {
        let sample_data_rows = file_info.sample_data.total_rows;

        let tab_manager = TabManager::new(
            file_info.schema.column_size(),
            file_info.row_groups.num_row_groups(),
            sample_data_rows,
            file_info.metadata.total_property_display_lines(),
        );

        Self {
            parquet_ctx: file_info,
            file_name: file_info.file_path.clone(),
            exit: false,
            tabs: tab_manager,
            state: AppState::new(),
        }
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.exit {
            // Calculate visible data rows based on terminal size
            let terminal_size = terminal.size()?;
            // Account for: header (3 lines), footer (1 line), table header (3 lines) = 7 lines total
            let visible_data_rows = (terminal_size.height.saturating_sub(7) as usize).max(1);
            self.state.set_visible_data_rows(visible_data_rows);

            // Lazily (re)load the row window backing the Visualize table so only a
            // bounded slice of a large file is ever held in memory.
            if self.tabs.active_tab().to_string() == "Visualize" {
                self.parquet_ctx
                    .sample_data
                    .ensure_loaded(self.state.data_vertical_scroll(), visible_data_rows);
            }

            // Bound horizontal column scrolling on the Visualize tab to what
            // actually fits, so it can't overshoot the last visible column (which
            // left phantom offset, causing "empty" presses when scrolling back).
            // The data table spans the full terminal width, so it is the width we
            // pass here. Other tabs keep their own bounds (unbounded here).
            let max_horizontal_offset = if self.tabs.active_tab().to_string() == "Visualize" {
                crate::components::DataTable::new(&self.parquet_ctx.sample_data)
                    .with_vertical_scroll(self.state.data_vertical_scroll())
                    .max_horizontal_scroll(terminal_size.width)
            } else {
                usize::MAX
            };
            self.state.set_max_horizontal_offset(max_horizontal_offset);

            let render_view = AppRenderView::from_app(self);
            terminal.draw(|frame| crate::ui::render_app(&render_view, frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> io::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event)
            }
            _ => {}
        };
        Ok(())
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => self.exit(),
            KeyCode::Esc => self.state.reset(),
            KeyCode::Tab => {
                self.tabs.next();
                self.state.reset();
            }
            KeyCode::BackTab => {
                self.tabs.prev();
                self.state.reset();
            }
            _ => {
                self.tabs
                    .active_tab()
                    .on_event(key_event, &mut self.state)
                    .unwrap();
            }
        }
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_right_is_clamped_to_max_horizontal_offset() {
        let mut state = AppState::new();
        state.set_max_horizontal_offset(3);
        for _ in 0..10 {
            state.right();
        }
        // Without the clamp the offset would run past the visible range,
        // producing the "empty clicks" seen when scrolling back.
        assert_eq!(state.horizontal_offset(), 3);
    }

    #[test]
    fn test_left_saturates_at_zero() {
        let mut state = AppState::new();
        state.set_max_horizontal_offset(5);
        state.right();
        state.right();
        state.left();
        state.left();
        state.left();
        assert_eq!(state.horizontal_offset(), 0);
    }

    #[test]
    fn test_jump_to_bottom_then_top() {
        let mut state = AppState::new();

        state.jump_to_bottom(20, 7300);
        assert_eq!(state.vertical_offset(), 7299);
        assert_eq!(state.data_vertical_scroll(), 7300 - 20);

        state.jump_to_top();
        assert_eq!(state.vertical_offset(), 0);
        assert_eq!(state.data_vertical_scroll(), 0);
    }

    #[test]
    fn test_shrinking_the_max_clamps_the_current_offset() {
        let mut state = AppState::new();
        state.set_max_horizontal_offset(10);
        for _ in 0..10 {
            state.right();
        }
        assert_eq!(state.horizontal_offset(), 10);
        // e.g. the terminal was widened, so fewer columns need scrolling.
        state.set_max_horizontal_offset(4);
        assert_eq!(state.horizontal_offset(), 4);
    }
}
