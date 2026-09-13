use std::cell::{Ref, RefCell};

use polars::prelude::*;

/// String cells for a slice of rows, paired with the (flattened) column names.
type LoadedRows = (Vec<Vec<String>>, Vec<String>);

/// Hard cap on the number of rows materialized in memory at any time.
const WINDOW_ROWS: usize = 2000;
/// Rows kept before the viewport when a reload is triggered, so scrolling back
/// up a little does not immediately force another reload.
const ROW_PREFETCH: usize = 500;

/// The currently materialized slice of rows: `rows[i]` is the file row at
/// absolute index `start + i`.
#[derive(Debug, Default)]
pub struct RowWindow {
    pub start: usize,
    pub rows: Vec<Vec<String>>,
}

impl RowWindow {
    /// Whether `[view_start, view_start + view_len)` is fully covered.
    fn covers(&self, view_start: usize, view_len: usize) -> bool {
        view_start >= self.start && view_start + view_len <= self.start + self.rows.len()
    }
}

/// A lazily-loaded, memory-bounded view over a parquet file's rows.
///
/// Only a sliding window of at most [`WINDOW_ROWS`] rows is held in memory; the
/// window is reloaded from the file (via a pushed-down slice) whenever the
/// viewport scrolls outside it. Column names and the total row count are known
/// up front from the file's schema/metadata.
#[derive(Debug)]
pub struct ParquetSampleData {
    file_path: String,
    pub flattened_columns: Vec<String>,
    pub total_columns: usize,
    pub total_rows: usize,
    window: RefCell<RowWindow>,
}

impl ParquetSampleData {
    /// Open `file_path` and load the first window of rows. `total_rows` is the
    /// row count from the parquet metadata (the caller already has it).
    pub fn open(
        file_path: &str,
        total_rows: usize,
    ) -> Result<ParquetSampleData, Box<dyn std::error::Error>> {
        let first_len = WINDOW_ROWS.min(total_rows);
        let (rows, columns) = if first_len > 0 {
            Self::load(file_path, 0, first_len)?
        } else {
            // Empty file: take column names from the lazy schema, no data read.
            let mut lf = LazyFrame::scan_parquet(PlPath::new(file_path), Default::default())?;
            let schema = lf.collect_schema()?;
            let names = schema.iter_names().map(|s| s.to_string()).collect();
            (Vec::new(), names)
        };

        Ok(ParquetSampleData {
            file_path: file_path.to_string(),
            total_columns: columns.len(),
            flattened_columns: columns,
            total_rows,
            window: RefCell::new(RowWindow { start: 0, rows }),
        })
    }

    /// Read `len` rows starting at absolute row `offset`, returning the string
    /// cells and the (flattened) column names.
    ///
    /// polars pushes the slice into the parquet scan, so only the row groups
    /// overlapping `[offset, offset + len)` are read and decompressed.
    fn load(
        file_path: &str,
        offset: usize,
        len: usize,
    ) -> Result<LoadedRows, Box<dyn std::error::Error>> {
        let df = LazyFrame::scan_parquet(PlPath::new(file_path), Default::default())?
            .slice(offset as i64, len as IdxSize)
            .collect()?;

        // Flatten struct columns (currently a no-op; see below).
        let df = Self::flatten_struct_columns(df)?;

        let columns: Vec<String> = df
            .get_column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let mut rows = Vec::with_capacity(df.height());
        for row_idx in 0..df.height() {
            let mut row = Vec::with_capacity(columns.len());
            for col in df.get_columns() {
                let series = col.as_materialized_series();
                row.push(Self::get_value_as_string(series, row_idx));
            }
            rows.push(row);
        }

        Ok((rows, columns))
    }

    /// Ensure the window covers `[view_start, view_start + view_len)`, reloading
    /// from the file if it does not. A reload failure leaves the current window
    /// in place rather than panicking.
    pub fn ensure_loaded(&self, view_start: usize, view_len: usize) {
        let view_len = view_len.min(self.total_rows.saturating_sub(view_start));
        {
            if self.window.borrow().covers(view_start, view_len) {
                return;
            }
        }

        let new_start = view_start.saturating_sub(ROW_PREFETCH);
        let len = WINDOW_ROWS.min(self.total_rows.saturating_sub(new_start));
        if len == 0 {
            return;
        }

        if let Ok((rows, _columns)) = Self::load(&self.file_path, new_start, len) {
            let mut window = self.window.borrow_mut();
            window.start = new_start;
            window.rows = rows;
        }
    }

    /// The currently materialized row window.
    pub fn loaded(&self) -> Ref<'_, RowWindow> {
        self.window.borrow()
    }

    fn flatten_struct_columns(df: DataFrame) -> Result<DataFrame, Box<dyn std::error::Error>> {
        // For now, we'll just return the dataframe as-is.
        // Struct columns are displayed with their string representation.
        // TODO: Add proper struct flattening if needed. If flattening changes the
        // column set, `load`'s projection (a future optimization) will need a
        // flattened -> physical name map.
        Ok(df)
    }

    fn get_value_as_string(col: &Series, row_idx: usize) -> String {
        match col.get(row_idx) {
            Ok(any_value) => {
                if any_value.is_null() {
                    "NULL".to_string()
                } else {
                    format!("{any_value}")
                }
            }
            Err(_) => "NULL".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_path(filename: &str) -> String {
        format!("{}/{}", crate::file::parquet_test_data(), filename)
    }

    // alltypes_tiny_pages.parquet: 7300 rows, 13 columns — larger than WINDOW_ROWS.
    fn wide_file() -> String {
        test_data_path("alltypes_tiny_pages.parquet")
    }

    #[test]
    fn open_reports_full_metadata_not_just_the_window() {
        let data = ParquetSampleData::open(&wide_file(), 7300).unwrap();

        assert_eq!(data.total_rows, 7300);
        assert_eq!(data.total_columns, 13);
        assert_eq!(data.flattened_columns.len(), data.total_columns);
        // Only a bounded window is actually materialized.
        assert_eq!(data.loaded().rows.len(), WINDOW_ROWS);
        assert_eq!(data.loaded().start, 0);
    }

    #[test]
    fn ensure_loaded_slides_the_window_for_a_far_scroll() {
        let data = ParquetSampleData::open(&wide_file(), 7300).unwrap();

        data.ensure_loaded(6000, 40);

        let win = data.loaded();
        assert!(win.start > 0, "window should have moved");
        assert_eq!(win.start, 6000 - ROW_PREFETCH);
        // The requested viewport is covered.
        assert!(win.covers(6000, 40));
        // Still bounded (only 7300 - start rows remain, but never more than the cap).
        assert!(win.rows.len() <= WINDOW_ROWS);
    }

    #[test]
    fn ensure_loaded_is_a_noop_inside_the_window() {
        let data = ParquetSampleData::open(&wide_file(), 7300).unwrap();
        let start_before = data.loaded().start;

        data.ensure_loaded(50, 40);

        assert_eq!(data.loaded().start, start_before);
    }

    #[test]
    fn windowed_rows_match_a_direct_read_at_the_same_offset() {
        let data = ParquetSampleData::open(&wide_file(), 7300).unwrap();
        data.ensure_loaded(6000, 10);

        let (direct, _) = ParquetSampleData::load(&wide_file(), 6000, 10).unwrap();
        let win = data.loaded();
        let rel = 6000 - win.start;
        assert_eq!(&win.rows[rel..rel + 10], &direct[..]);
    }
}
