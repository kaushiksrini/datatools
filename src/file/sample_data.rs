use polars::prelude::*;

const MAX_ROWS: usize = 200;

#[derive(Debug, Clone, Default)]
pub struct ParquetSampleData {
    pub flattened_columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub total_columns: usize,
    pub total_rows: usize,
}

// TODO: in future create a independent crate that does the parsing,
// the polars crate is large and doesn't support complex nested types.
impl ParquetSampleData {
    /// Read a preview of `file_path`, catching panics and converting them to errors.
    pub fn read_sample_data(
        file_path: &str,
    ) -> Result<ParquetSampleData, Box<dyn std::error::Error>> {
        let path = file_path.to_string();

        // Temporarily silence the default hook while polars runs, dangerous!
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(move || Self::read_with_polars(&path));
        std::panic::set_hook(previous_hook);

        match result {
            Ok(res) => res.map_err(Into::into),
            Err(payload) => Err(panic_message(&payload).into()),
        }
    }

    fn read_with_polars(file_path: &str) -> PolarsResult<ParquetSampleData> {
        // Read parquet file using polars LazyFrame
        let df = LazyFrame::scan_parquet(PlRefPath::new(file_path), Default::default())?
            .limit(MAX_ROWS as u32)
            .collect()?;

        // Flatten struct columns
        let df = Self::flatten_struct_columns(df);

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

    fn flatten_struct_columns(df: DataFrame) -> DataFrame {
        // For now, we'll just return the dataframe as-is
        // Struct columns will be displayed with their string representation
        // TODO: Add proper struct flattening if needed
        df
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

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "polars panicked while reading this file".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_path(filename: &str) -> String {
        format!("{}/{}", crate::file::parquet_test_data(), filename)
    }

    #[test]
    fn reads_a_plain_file() {
        let data =
            ParquetSampleData::read_sample_data(&test_data_path("alltypes_plain.parquet")).unwrap();

        assert_eq!(data.total_columns, 11);
        assert_eq!(data.total_rows, 8);
    }

    /// Write a parquet file whose column carries `geoarrow.wkb` arrow extension
    /// metadata. None of the checked-in files use extension types, so the file
    /// is generated here to keep the regression self-contained.
    fn write_geoarrow_wkb_file() -> std::path::PathBuf {
        use arrow::array::{ArrayRef, BinaryArray, RecordBatch};
        use arrow::datatypes::{DataType, Field, Schema};
        use parquet::arrow::ArrowWriter;
        use std::collections::HashMap;
        use std::sync::Arc;

        let field = Field::new("geom", DataType::Binary, true).with_metadata(HashMap::from([
            (
                "ARROW:extension:name".to_string(),
                "geoarrow.wkb".to_string(),
            ),
            (
                "ARROW:extension:metadata".to_string(),
                r#"{"crs":"EPSG:4326"}"#.to_string(),
            ),
        ]));
        let schema = Arc::new(Schema::new(vec![field]));
        let values: ArrayRef = Arc::new(BinaryArray::from_opt_vec(vec![
            Some(&[0x01, 0x01, 0x00, 0x00, 0x00][..]),
            None,
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![values]).unwrap();

        let path = std::env::temp_dir().join(format!(
            "parqeye_geoarrow_wkb_{}.parquet",
            std::process::id()
        ));
        let mut writer =
            ArrowWriter::try_new(std::fs::File::create(&path).unwrap(), schema, None).unwrap();
        writer.write(&batch).unwrap();
        writer.close().unwrap();

        path
    }

    #[test]
    fn reads_arrow_extension_types() {
        // Without polars' `dtype-extension` feature this panicked in polars'
        // catch-all arm and took the TUI down.
        let path = write_geoarrow_wkb_file();

        let data = ParquetSampleData::read_sample_data(path.to_str().unwrap()).unwrap();

        assert_eq!(data.flattened_columns, vec!["geom"]);
        assert_eq!(data.total_rows, 2);
        assert_eq!(data.rows[1][0], "NULL");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unsupported_file_errors_instead_of_panicking() {
        // GEOMETRY/GEOGRAPHY logical types are rejected by polars' Thrift parser.
        let err =
            ParquetSampleData::read_sample_data(&test_data_path("geospatial/crs-srid.parquet"))
                .unwrap_err();

        assert!(
            err.to_string().contains("LogicalType"),
            "unexpected error: {err}"
        );
    }
}
