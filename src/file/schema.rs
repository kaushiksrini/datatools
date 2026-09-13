use std::{cmp::Ordering, collections::HashSet};

use parquet::basic::{LogicalType, TimeUnit, Type as PhysicalType};
use parquet::file::metadata::ParquetMetaData;
use parquet::schema::types::Type as ParquetType;
use ratatui::{
    style::{Color, Stylize},
    widgets::{Cell, Row},
};

use crate::file::utils::format_size;

#[derive(Debug, Clone)]
pub struct ColumnStats {
    pub min: Option<String>,
    pub max: Option<String>,
    pub nulls: u64,
    pub distinct: Option<u64>,
    pub total_compressed_size: u64,
    pub total_uncompressed_size: u64,
}

#[derive(Clone)]
pub struct ColumnSchemaInfo {
    pub name: String,
    pub repetition: String,
    pub physical: String,
    pub logical: String,
    pub codec: String,
    pub converted_type: String,
    pub encoding: String,
    pub dictionary_values: Option<Vec<String>>,
}

#[derive(Clone)]
pub enum SchemaInfo {
    Root {
        name: String,
        display: String,
    },
    Primitive {
        name: String,
        display: String,
        info: Box<ColumnSchemaInfo>,
        stats: ColumnStats,
    },
    Group {
        name: String,
        display: String,
        repetition: String,
    },
}

// TODO: Add Dictionary Values
pub struct FileSchema {
    pub columns: Vec<SchemaInfo>,
}

impl FileSchema {
    pub fn from_metadata(md: &ParquetMetaData) -> Result<FileSchema, Box<dyn std::error::Error>> {
        let schema_descr: &parquet::schema::types::SchemaDescriptor =
            md.file_metadata().schema_descr();
        let root = schema_descr.root_schema();

        // Pre-compute codec + encoding summary for every leaf column
        let mut summaries: Vec<(String, String)> = Vec::new();
        for (col_idx, _) in schema_descr.columns().iter().enumerate() {
            // use std::collections::BTreeSet;
            let mut codecs: HashSet<String> = HashSet::new();
            let mut encs: HashSet<String> = HashSet::new();

            md.row_groups().iter().for_each(|rg| {
                let col_chunk = rg.column(col_idx);
                codecs.insert(format!("{:?}", col_chunk.compression()));
                encs.extend(col_chunk.encodings().map(|enc| format!("{enc:?}")));
            });

            let codec_summary = codecs.into_iter().collect::<Vec<_>>().join(", ");
            let enc_summary = encs.into_iter().collect::<Vec<_>>().join(", ");

            summaries.push((codec_summary, enc_summary));
        }

        let mut lines: Vec<SchemaInfo> = Vec::new();
        lines.push(SchemaInfo::Root {
            name: "root".to_string(),
            display: "└─ root".to_string(),
        });

        let children = root.get_fields();
        let count = children.len();
        let mut leaf_idx: usize = 0;

        for (idx, child) in children.iter().enumerate() {
            traverse(
                child.as_ref(),
                "   ".to_string(),
                idx == count - 1,
                &mut lines,
                &mut leaf_idx,
                &summaries,
                md,
            );
        }

        Ok(FileSchema { columns: lines })
    }

    pub fn column_group_name(&self, index: usize) -> String {
        match self.columns.get(index).unwrap() {
            SchemaInfo::Primitive { name, .. } => name.clone(),
            SchemaInfo::Group { name, .. } => name.clone(),
            _ => unreachable!(),
        }
    }

    pub fn column_size(&self) -> usize {
        self.columns
            .iter()
            .filter(|c| matches!(c, SchemaInfo::Primitive { .. }))
            .count()
    }

    pub fn tree_width(&self) -> usize {
        self.columns
            .iter()
            .map(|c| match c {
                SchemaInfo::Root { display, .. } => display.len(),
                SchemaInfo::Primitive { display, .. } => display.len(),
                SchemaInfo::Group { display, .. } => display.len(),
            })
            .max()
            .unwrap_or(0)
            .max(24) // max for the bottom of the chart
    }

    pub fn primitive_column_names(&self) -> Vec<String> {
        self.columns
            .iter()
            .filter(|c| matches!(c, SchemaInfo::Primitive { .. }))
            .map(|c| match c {
                SchemaInfo::Primitive { name, .. } => name.clone(),
                _ => unreachable!(),
            })
            .collect()
    }

    pub fn generate_table_rows(&self, selected_index: Option<usize>) -> Vec<Row<'_>> {
        let mut primitive_index = 1; // Start counting primitives from 1 (like app does)

        self.columns
            .iter()
            .filter_map(|col| {
                if let SchemaInfo::Primitive { info, stats, .. } = col {
                    let compression_ratio = if stats.total_uncompressed_size > 0 {
                        format!(
                            "{:.2}x",
                            stats.total_uncompressed_size as f64
                                / stats.total_compressed_size as f64
                        )
                    } else {
                        "N/A".to_string()
                    };

                    let is_selected = selected_index == Some(primitive_index);

                    let mut row = Row::new([
                        Cell::from(info.repetition.clone()),
                        Cell::from(info.physical.clone()),
                        Cell::from(format_size(stats.total_compressed_size)),
                        Cell::from(format_size(stats.total_uncompressed_size)),
                        Cell::from(compression_ratio),
                        Cell::from(info.encoding.clone()),
                        Cell::from(info.codec.clone()),
                        Cell::from(stats.min.clone().unwrap_or_else(|| "NULL".to_string())),
                        Cell::from(stats.max.clone().unwrap_or_else(|| "NULL".to_string())),
                        Cell::from(stats.nulls.to_string()),
                    ]);

                    if is_selected {
                        row = row.style(
                            ratatui::style::Style::default()
                                .bg(Color::Yellow)
                                .fg(Color::Black),
                        );
                    }

                    primitive_index += 1;
                    Some(row)
                } else if let SchemaInfo::Group { repetition, .. } = col {
                    let row = Row::new(vec![
                        Cell::from(repetition.clone().green()),
                        Cell::from("group".green()),
                    ]);
                    Some(row)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn generate_table_rows_with_columns(
        &self,
        selected_index: usize,
        start_col: usize,
        num_cols: usize,
    ) -> (Vec<Row<'_>>, Vec<usize>) {
        self.generate_table_rows_with_scroll(
            selected_index,
            start_col,
            num_cols,
            0,
            self.columns.len(),
        )
    }

    pub fn generate_table_rows_with_scroll(
        &self,
        selected_index: usize,
        start_col: usize,
        num_cols: usize,
        start_row: usize,
        num_rows: usize,
    ) -> (Vec<Row<'_>>, Vec<usize>) {
        let mut primitive_index = 1; // Start counting primitives from 1 (like app does)
        let mut column_widths = vec![0usize; num_cols];

        let rows = self
            .columns
            .iter()
            .enumerate()
            .skip(start_row + 1)
            .take(num_rows)
            .filter_map(|(_col_idx, col)| {
                if let SchemaInfo::Primitive { info, stats, .. } = col {
                    let compression_ratio = if stats.total_uncompressed_size > 0 {
                        format!(
                            "{:.2}x",
                            stats.total_uncompressed_size as f64
                                / stats.total_compressed_size as f64
                        )
                    } else {
                        "N/A".to_string()
                    };

                    let is_selected =
                        selected_index > 0 && (selected_index - start_row) == primitive_index;

                    // Create all cells first
                    let all_cells = vec![
                        info.repetition.clone(),
                        info.physical.clone(),
                        format_size(stats.total_compressed_size),
                        format_size(stats.total_uncompressed_size),
                        compression_ratio,
                        info.encoding.clone(),
                        info.codec.clone(),
                        stats.min.clone().unwrap_or_else(|| "NULL".to_string()),
                        stats.max.clone().unwrap_or_else(|| "NULL".to_string()),
                        stats.nulls.to_string(),
                    ];

                    // Select only the visible columns and track their content lengths
                    let visible_cell_contents: Vec<_> = all_cells
                        .into_iter()
                        .skip(start_col)
                        .take(num_cols)
                        .collect();

                    // Update column widths with the maximum seen so far
                    for (col_idx, content) in visible_cell_contents.iter().enumerate() {
                        column_widths[col_idx] = column_widths[col_idx].max(content.len());
                    }

                    // Create cells from the content
                    let visible_cells: Vec<_> =
                        visible_cell_contents.into_iter().map(Cell::from).collect();

                    let mut row = Row::new(visible_cells);

                    if is_selected {
                        row = row.style(
                            ratatui::style::Style::default()
                                .bg(Color::Yellow)
                                .fg(Color::Black),
                        );
                    }

                    primitive_index += 1;
                    Some(row)
                } else if let SchemaInfo::Group { repetition, .. } = col {
                    let all_cells = vec![
                        repetition.clone(),
                        "group".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                        "".to_string(),
                    ];

                    let visible_cell_contents: Vec<_> = all_cells
                        .into_iter()
                        .skip(start_col)
                        .take(num_cols)
                        .collect();

                    // Update column widths with the maximum seen so far
                    for (col_idx, content) in visible_cell_contents.iter().enumerate() {
                        column_widths[col_idx] = column_widths[col_idx].max(content.len());
                    }

                    let visible_cells: Vec<_> = visible_cell_contents
                        .into_iter()
                        .enumerate()
                        .map(|(idx, content)| {
                            if idx == 0 || idx == 1 {
                                Cell::from(content.green())
                            } else {
                                Cell::from(content)
                            }
                        })
                        .collect();

                    let row = Row::new(visible_cells);
                    Some(row)
                } else {
                    None
                }
            })
            .collect();

        (rows, column_widths)
    }
}

fn traverse(
    node: &ParquetType,
    prefix: String,
    is_last: bool,
    lines: &mut Vec<SchemaInfo>,
    leaf_idx: &mut usize,
    summaries: &Vec<(String, String)>,
    md: &ParquetMetaData,
) {
    let connector: &'static str = if is_last { "└─" } else { "├─" };
    let line = format!("{}{} {}", prefix, connector, node.name());

    if node.is_primitive() {
        let repetition = format!("{:?}", node.get_basic_info().repetition());
        let physical = format!("{:?}", node.get_physical_type());
        let logical = match node.get_basic_info().logical_type_ref() {
            Some(logical_type) => logical_type_to_string(logical_type),
            None => String::new(),
        };

        let (codec_sum, enc_sum) = &summaries[*leaf_idx];
        let stats = aggregate_column_stats(md, *leaf_idx, node.get_physical_type());
        let info = ColumnSchemaInfo {
            name: node.name().to_string(),
            repetition: repetition.clone(),
            physical: physical.clone(),
            logical: logical.clone(),
            codec: codec_sum.clone(),
            encoding: enc_sum.clone(),
            converted_type: node.get_basic_info().converted_type().to_string(),
            dictionary_values: None,
        };
        lines.push(SchemaInfo::Primitive {
            name: node.name().to_string(),
            display: line,
            info: Box::new(info),
            stats,
        });

        *leaf_idx += 1;
    } else {
        lines.push(SchemaInfo::Group {
            name: node.name().to_string(),
            display: line,
            repetition: format!("{:?}", node.get_basic_info().repetition()),
        });
    }

    if node.is_group() {
        let fields = node.get_fields();
        let count = fields.len();
        for (idx, child) in fields.iter().enumerate() {
            let next_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });
            traverse(
                child.as_ref(),
                next_prefix,
                idx == count - 1,
                lines,
                leaf_idx,
                summaries,
                md,
            );
        }
    }
}

/// Efficiently aggregate column statistics across all row groups
fn aggregate_column_stats(
    md: &ParquetMetaData,
    col_idx: usize,
    physical: PhysicalType,
) -> ColumnStats {
    let (min_bytes, max_bytes, nulls, distinct, total_compressed_size, total_uncompressed_size) =
        md.row_groups().iter().fold(
            (
                None::<Vec<u8>>,
                None::<Vec<u8>>,
                0u64,
                None::<u64>,
                0u64,
                0u64,
            ),
            |(
                mut min_bytes,
                mut max_bytes,
                mut nulls,
                mut distinct,
                mut compressed,
                mut uncompressed,
            ),
             rg| {
                let col_meta = rg.column(col_idx);
                if let Some(stats) = col_meta.statistics() {
                    nulls += stats.null_count_opt().unwrap_or(0);
                    distinct =
                        Some(distinct.unwrap_or(0) + stats.distinct_count_opt().unwrap_or(0));

                    if let Some(min_b) = stats.min_bytes_opt()
                        && min_bytes.as_ref().is_none_or(|mb| {
                            compare_stat_values(min_b, mb, physical) == Ordering::Less
                        })
                    {
                        min_bytes = Some(min_b.to_vec());
                    }
                    if let Some(max_b) = stats.max_bytes_opt()
                        && max_bytes.as_ref().is_none_or(|mb| {
                            compare_stat_values(max_b, mb, physical) == Ordering::Greater
                        })
                    {
                        max_bytes = Some(max_b.to_vec());
                    }
                }
                compressed += col_meta.compressed_size() as u64;
                uncompressed += col_meta.uncompressed_size() as u64;
                (
                    min_bytes,
                    max_bytes,
                    nulls,
                    distinct,
                    compressed,
                    uncompressed,
                )
            },
        );

    ColumnStats {
        min: min_bytes.as_deref().map(|b| decode_value(b, physical)),
        max: max_bytes.as_deref().map(|b| decode_value(b, physical)),
        nulls,
        distinct,
        total_compressed_size,
        total_uncompressed_size,
    }
}

/*
   Compare statistical values based on physical type;
   avoid directly comparing the encoded bytes of integers and floating-point numbers.
*/
fn compare_stat_values(left: &[u8], right: &[u8], physical: PhysicalType) -> Ordering {
    match physical {
        PhysicalType::INT32 if left.len() == 4 && right.len() == 4 => {
            i32::from_le_bytes(left.try_into().unwrap())
                .cmp(&i32::from_le_bytes(right.try_into().unwrap()))
        }
        PhysicalType::INT64 if left.len() == 8 && right.len() == 8 => {
            i64::from_le_bytes(left.try_into().unwrap())
                .cmp(&i64::from_le_bytes(right.try_into().unwrap()))
        }
        PhysicalType::FLOAT if left.len() == 4 && right.len() == 4 => {
            f32::from_le_bytes(left.try_into().unwrap())
                .total_cmp(&f32::from_le_bytes(right.try_into().unwrap()))
        }
        PhysicalType::DOUBLE if left.len() == 8 && right.len() == 8 => {
            f64::from_le_bytes(left.try_into().unwrap())
                .total_cmp(&f64::from_le_bytes(right.try_into().unwrap()))
        }
        _ => left.cmp(right),
    }
}

/// Decode raw statistics bytes into a readable value based on the physical type
fn decode_value(bytes: &[u8], physical: PhysicalType) -> String {
    match physical {
        PhysicalType::INT32 if bytes.len() == 4 => {
            i32::from_le_bytes(bytes.try_into().unwrap()).to_string()
        }
        PhysicalType::INT64 if bytes.len() == 8 => {
            i64::from_le_bytes(bytes.try_into().unwrap()).to_string()
        }
        PhysicalType::FLOAT if bytes.len() == 4 => {
            format!("{:.4}", f32::from_le_bytes(bytes.try_into().unwrap()))
        }
        PhysicalType::DOUBLE if bytes.len() == 8 => {
            format!("{:.4}", f64::from_le_bytes(bytes.try_into().unwrap()))
        }
        PhysicalType::BYTE_ARRAY | PhysicalType::FIXED_LEN_BYTE_ARRAY => std::str::from_utf8(bytes)
            .map_or_else(
                |_| {
                    bytes
                        .iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join("")
                },
                |s| s.to_string(),
            ),
        _ => bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(""),
    }
}

fn logical_type_to_string(logical_type: &LogicalType) -> String {
    match logical_type {
        LogicalType::Decimal(t) => format!("Decimal({},{})", t.scale, t.precision),
        LogicalType::Integer(t) => format!(
            "Integer({},{})",
            t.bit_width,
            if t.is_signed { "sign" } else { "unsign" }
        ),
        LogicalType::Time(t) => match t.unit {
            TimeUnit::MILLIS => format!(
                "Time({}, millis)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
            TimeUnit::MICROS => format!(
                "Time({}, micros)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
            TimeUnit::NANOS => format!(
                "Time({}, nanos)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
        },
        LogicalType::Timestamp(t) => match t.unit {
            TimeUnit::MILLIS => format!(
                "Timestamp({}, millis)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
            TimeUnit::MICROS => format!(
                "Timestamp({}, micros)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
            TimeUnit::NANOS => format!(
                "Timestamp({}, nanos)",
                if t.is_adjusted_to_u_t_c {
                    "utc"
                } else {
                    "local"
                }
            ),
        },
        _ => format!("{logical_type:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parquet::file::reader::{FileReader, SerializedFileReader};
    use std::fs::File;

    fn load_alltypes_schema() -> FileSchema {
        let path = format!(
            "{}/alltypes_plain.parquet",
            crate::file::parquet_test_data(),
        );
        let file = File::open(path).unwrap();
        let reader = SerializedFileReader::try_from(file).unwrap();
        let metadata = reader.metadata();
        FileSchema::from_metadata(metadata).unwrap()
    }

    #[test]
    fn test_file_alltypes_plain_basic() {
        let file_schema = load_alltypes_schema();

        // Test basic metrics
        assert_eq!(11, file_schema.column_size());
        assert_eq!(25, file_schema.tree_width());

        // Should have root + 11 primitive columns = 12 total
        assert!(file_schema.columns.len() >= 12);
    }

    #[test]
    fn test_primitive_column_names() {
        let file_schema = load_alltypes_schema();
        let names = file_schema.primitive_column_names();

        // alltypes_plain.parquet has these columns
        assert_eq!(11, names.len());
        assert!(names.contains(&"id".to_string()));
        assert!(names.contains(&"bool_col".to_string()));
        assert!(names.contains(&"tinyint_col".to_string()));
        assert!(names.contains(&"smallint_col".to_string()));
        assert!(names.contains(&"int_col".to_string()));
        assert!(names.contains(&"bigint_col".to_string()));
        assert!(names.contains(&"float_col".to_string()));
        assert!(names.contains(&"double_col".to_string()));
        assert!(names.contains(&"date_string_col".to_string()));
        assert!(names.contains(&"string_col".to_string()));
        assert!(names.contains(&"timestamp_col".to_string()));
    }

    #[test]
    fn test_column_group_name() {
        let file_schema = load_alltypes_schema();

        // First column after root should be 'id'
        assert_eq!("id", file_schema.column_group_name(1));

        // Get a few more column names
        let names = file_schema.primitive_column_names();
        assert!(!names.is_empty());
    }

    #[test]
    fn test_column_stats() {
        let file_schema = load_alltypes_schema();

        // Find the 'id' column and check its stats
        for col in &file_schema.columns {
            if let SchemaInfo::Primitive { name, stats, .. } = col
                && name == "id"
            {
                // alltypes_plain has 8 rows with id from 0 to 7
                assert_eq!(stats.min, None);
                assert_eq!(stats.max, None);
                assert_eq!(stats.nulls, 0);

                // Should have compression stats
                assert!(stats.total_compressed_size > 0);
                assert!(stats.total_uncompressed_size > 0);
            }
        }
    }

    #[test]
    fn test_column_schema_info() {
        let file_schema = load_alltypes_schema();

        // Check the schema info for specific columns
        for col in &file_schema.columns {
            if let SchemaInfo::Primitive { name, info, .. } = col {
                match name.as_str() {
                    "id" => {
                        assert_eq!(info.physical, "INT32");
                        assert!(!info.repetition.is_empty());
                    }
                    "bool_col" => {
                        assert_eq!(info.physical, "BOOLEAN");
                    }
                    "float_col" => {
                        assert_eq!(info.physical, "FLOAT");
                    }
                    "double_col" => {
                        assert_eq!(info.physical, "DOUBLE");
                    }
                    "bigint_col" => {
                        assert_eq!(info.physical, "INT64");
                    }
                    "string_col" | "date_string_col" => {
                        assert_eq!(info.physical, "BYTE_ARRAY");
                    }
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn test_generate_table_rows() {
        let file_schema = load_alltypes_schema();

        // Generate rows with no selection
        let rows = file_schema.generate_table_rows(None);

        // Should have 11 primitive columns
        assert_eq!(11, rows.len());

        // Generate rows with selection
        let rows_selected = file_schema.generate_table_rows(Some(1));
        assert_eq!(11, rows_selected.len());
    }

    #[test]
    fn test_generate_table_rows_with_columns() {
        let file_schema = load_alltypes_schema();

        // Test with different column ranges
        let (rows, widths) = file_schema.generate_table_rows_with_columns(1, 0, 5);

        // Should have rows for primitive columns
        assert!(!rows.is_empty());

        // Should have width info for 5 columns
        assert_eq!(5, widths.len());

        // All widths should be non-zero (content should exist)
        for width in &widths {
            assert!(*width > 0);
        }
    }

    #[test]
    fn test_generate_table_rows_with_scroll() {
        let file_schema = load_alltypes_schema();

        // Test scrolling with start_row and limited rows
        let (rows, widths) = file_schema.generate_table_rows_with_scroll(
            1,  // selected_index
            0,  // start_col
            10, // num_cols
            0,  // start_row
            5,  // num_rows (limit to 5)
        );

        // Should have at most 5 rows
        assert!(rows.len() <= 5);

        // Should have width info for 10 columns
        assert_eq!(10, widths.len());
    }

    #[test]
    fn test_schema_info_types() {
        let file_schema = load_alltypes_schema();

        // First item should be root
        assert!(matches!(&file_schema.columns[0], SchemaInfo::Root { .. }));

        // Count different types
        let mut root_count = 0;
        let mut primitive_count = 0;
        let mut group_count = 0;

        for col in &file_schema.columns {
            match col {
                SchemaInfo::Root { .. } => root_count += 1,
                SchemaInfo::Primitive { .. } => primitive_count += 1,
                SchemaInfo::Group { .. } => group_count += 1,
            }
        }

        assert_eq!(1, root_count);
        assert_eq!(11, primitive_count);
        // alltypes_plain doesn't have nested groups (flat schema)
        assert_eq!(0, group_count);
    }

    #[test]
    fn test_column_display_strings() {
        let file_schema = load_alltypes_schema();

        // Check that display strings are properly formatted
        for col in &file_schema.columns {
            match col {
                SchemaInfo::Root { display, .. } => {
                    assert!(display.contains("root"));
                }
                SchemaInfo::Primitive { name, display, .. } => {
                    assert!(display.contains(name));
                }
                SchemaInfo::Group { name, display, .. } => {
                    assert!(display.contains(name));
                }
            }
        }
    }

    #[test]
    fn test_compression_ratio_calculation() {
        let file_schema = load_alltypes_schema();

        // Check that compression ratios are calculated correctly
        for col in &file_schema.columns {
            if let SchemaInfo::Primitive { stats, .. } = col
                && stats.total_uncompressed_size > 0
                && stats.total_compressed_size > 0
            {
                let ratio =
                    stats.total_uncompressed_size as f64 / stats.total_compressed_size as f64;

                // Compression ratio should be reasonable (between 0.5x and 10x)
                assert!(ratio > 0.5 && ratio < 10.0);
            }
        }
    }

    #[test]
    fn test_decode_value_int32() {
        let value = decode_value(&[42, 0, 0, 0], PhysicalType::INT32);
        assert_eq!(value, "42");

        let negative = decode_value(&[255, 255, 255, 255], PhysicalType::INT32);
        assert_eq!(negative, "-1");
    }

    #[test]
    fn test_compare_stat_values_uses_numeric_order_for_int32() {
        assert_eq!(
            compare_stat_values(
                &(-1i32).to_le_bytes(),
                &1i32.to_le_bytes(),
                PhysicalType::INT32
            ),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_stat_values(
                &256i32.to_le_bytes(),
                &1i32.to_le_bytes(),
                PhysicalType::INT32
            ),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_compare_stat_values_uses_numeric_order_for_float() {
        assert_eq!(
            compare_stat_values(
                &(-1.5f32).to_le_bytes(),
                &1.5f32.to_le_bytes(),
                PhysicalType::FLOAT
            ),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn test_decode_value_int64() {
        let value = decode_value(&[42, 0, 0, 0, 0, 0, 0, 0], PhysicalType::INT64);
        assert_eq!(value, "42");
    }

    #[test]
    fn test_decode_value_float() {
        let bytes = std::f32::consts::PI.to_le_bytes();
        let value = decode_value(&bytes, PhysicalType::FLOAT);
        assert!(value.starts_with("3.14"));
    }

    #[test]
    fn test_decode_value_double() {
        let bytes = std::f64::consts::PI.to_le_bytes();
        let value = decode_value(&bytes, PhysicalType::DOUBLE);
        assert!(value.starts_with("3.141"));
    }

    #[test]
    fn test_decode_value_byte_array() {
        let text = "hello";
        let value = decode_value(text.as_bytes(), PhysicalType::BYTE_ARRAY);
        assert_eq!(value, "hello");

        // Test non-UTF8 bytes (should return hex)
        let binary = [0xFF, 0xFE, 0xFD];
        let value = decode_value(&binary, PhysicalType::BYTE_ARRAY);
        assert_eq!(value, "FFFEFD");
    }

    #[test]
    fn test_logical_type_to_string() {
        // Test Decimal
        let decimal = LogicalType::decimal(2, 10);
        assert_eq!(logical_type_to_string(&decimal), "Decimal(2,10)");

        // Test Integer
        let integer = LogicalType::integer(32, true);
        assert_eq!(logical_type_to_string(&integer), "Integer(32,sign)");

        let unsigned = LogicalType::integer(16, false);
        assert_eq!(logical_type_to_string(&unsigned), "Integer(16,unsign)");
    }
}
