use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Stylize;
use ratatui::text::Span;
use std::io;

use crate::{app::AppState, tabs::Tab};

pub struct VisualizeTab {
    pub max_horizontal_scroll: Option<usize>,
    pub max_rows: Option<usize>,
    pub visible_rows: Option<usize>,
}

impl Default for VisualizeTab {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualizeTab {
    pub fn new() -> Self {
        Self {
            max_horizontal_scroll: None,
            max_rows: None,
            visible_rows: None,
        }
    }

    pub fn with_max_horizontal_scroll(mut self, max_horizontal_scroll: usize) -> Self {
        self.max_horizontal_scroll = Some(max_horizontal_scroll);
        self
    }

    pub fn with_max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    pub fn with_visible_rows(mut self, visible_rows: usize) -> Self {
        self.visible_rows = Some(visible_rows);
        self
    }
}

impl Tab for VisualizeTab {
    fn on_event(&self, key_event: KeyEvent, state: &mut AppState) -> Result<(), io::Error> {
        let max_rows = self.max_rows.unwrap_or(0);
        let visible_rows = state.visible_data_rows();

        match key_event.code {
            // Row navigation (Up/Down arrows)
            KeyCode::Up => {
                if state.vertical_offset() > 0 {
                    state.up();
                    state.adjust_scroll_to_selection(visible_rows, max_rows);
                }
            }
            KeyCode::Down => {
                if state.vertical_offset() < max_rows.saturating_sub(1) {
                    state.down();
                    state.adjust_scroll_to_selection(visible_rows, max_rows);
                }
            }
            // Page navigation (u/d keys)
            KeyCode::Char('u') | KeyCode::Char('U') => {
                state.page_up(visible_rows, max_rows);
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                state.page_down(visible_rows, max_rows);
            }
            // Jump to first / last row (vim-style g/G, or Home/End)
            KeyCode::Char('g') | KeyCode::Home => state.jump_to_top(),
            KeyCode::Char('G') | KeyCode::End => {
                state.jump_to_bottom(visible_rows, max_rows);
            }
            // Column navigation (Left/Right arrows)
            KeyCode::Left if state.horizontal_offset() > 0 => state.left(),
            // Upper bound is enforced in AppState against the on-screen column
            // count each frame, so no guard is needed here.
            KeyCode::Right => state.right(),
            _ => {}
        }
        Ok(())
    }

    fn instructions(&self) -> Vec<Span<'static>> {
        vec![
            "↑".green(),
            "/".white(),
            "↓".blue(),
            " : ".into(),
            "Row".into(),
            " | ".white(),
            "→".green(),
            "/".white(),
            "←".blue(),
            " : ".into(),
            "Column".into(),
            " | ".white(),
            "u".green(),
            "/".white(),
            "d".blue(),
            " : ".into(),
            "Page".into(),
            " | ".white(),
            "g".green(),
            "/".white(),
            "G".blue(),
            " : ".into(),
            "Top/Bottom".into(),
        ]
    }

    fn to_string(&self) -> String {
        "Visualize".to_string()
    }
}
