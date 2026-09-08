use polars::prelude::*;

#[derive(Debug, Clone)]
pub struct ParquetSampleData {
    pub flattened_columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total_columns: usize,
    pub total_rows: usize,
}

// TODO: in future create a independent crate that does the parsing,
// the polars crate is large and doesn't support complex nested types.
impl ParquetSampleData {
    pub fn read_sample_data(
        file_path: &str,
    ) -> Result<ParquetSampleData, Box<dyn std::error::Error>> {
        const MAX_ROWS: usize = 200;

        // Read parquet file using polars LazyFrame
        let df = LazyFrame::scan_parquet(PlRefPath::new(file_path), Default::default())?
            .limit(MAX_ROWS as u32)
            .collect()?;

        // Flatten struct columns
        let df = Self::flatten_struct_columns(df)?;

        // Get column names
        let flattened_columns: Vec<String> = df
            .get_column_names()
            .iter()
            .map(|s| s.to_string())
            .collect();

        let total_columns = flattened_columns.len();

        // Convert dataframe to rows of strings
        let mut rows = Vec::new();
        for row_idx in 0..df.height() {
            let mut row = Vec::new();
            for col in df.columns() {
                let series = col.as_materialized_series();
                let value = Self::get_value_as_string(series, row_idx);
                row.push(value);
            }
            rows.push(row);
        }

        Ok(ParquetSampleData {
            total_columns,
            flattened_columns,
            rows,
            total_rows: df.height(),
        })
    }

    fn flatten_struct_columns(df: DataFrame) -> Result<DataFrame, Box<dyn std::error::Error>> {
        // For now, we'll just return the dataframe as-is
        // Struct columns will be displayed with their string representation
        // TODO: Add proper struct flattening if needed
        Ok(df)
    }

    fn get_value_as_string(col: &Series, row_idx: usize) -> String {
        // Use get() which returns AnyValue and handle it
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
