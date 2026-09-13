use parquet::basic::{Encoding, PageType};
use parquet::column::page::{Page, PageReader};
use parquet::file::metadata::{ColumnChunkMetaData, RowGroupMetaData};
use parquet::file::reader::FileReader;
use parquet::file::reader::{ChunkReader, SerializedFileReader};
use parquet::file::statistics::Statistics;

use itertools::Itertools;
use std::iter::Iterator;

#[derive(Default)]
pub struct RowGroupPageInfo {
    pub page_infos: Vec<PageInfo>,
}

pub struct HasStats {
    pub has_stats: bool,
    pub has_dictionary_page: bool,
    pub has_bloom_filter: bool,
    pub has_page_encoding_stats: bool,
}

pub struct PageInfo {
    pub page_type: String,
    pub size: usize,
    pub rows: usize,
    pub encoding: String,
}

pub struct RowGroupColumnStats {
    pub min: Option<String>,
    pub max: Option<String>,
    pub null_count: Option<u64>,
    pub distinct_count: Option<u64>,
}

pub struct RowGroupColumnMetadata {
    /// `None` when the writer did not populate the (deprecated) `file_offset` field.
    pub file_offset: Option<u64>,
    pub column_path: String,
    pub has_stats: HasStats,
    pub statistics: Option<RowGroupColumnStats>,
    pub total_compressed_size: i64,
    pub total_uncompressed_size: i64,
    pub compression_type: String,
}

pub struct RowGroupAvgMedianStats {
    pub avg_compressed_size: f64,
    pub median_compressed_size: f64,
    pub avg_uncompressed_size: f64,
    pub median_uncompressed_size: f64,
    pub avg_rows_per_rg: f64,
    pub median_rows_per_rg: f64,
    pub avg_compression_ratio: f64,
    pub median_compression_ratio: f64,
}

impl RowGroupAvgMedianStats {
    pub fn new(row_groups_stats: &[RowGroupStats]) -> Self {
        // TODO: parallelize or iterate row group stats once
        Self {
            avg_compressed_size: row_groups_stats
                .iter()
                .map(|rg| rg.compressed_size)
                .sum::<i64>() as f64
                / row_groups_stats.len() as f64,
            median_compressed_size: row_groups_stats
                .iter()
                .map(|rg| rg.compressed_size)
                .sorted()
                .nth(row_groups_stats.len() / 2)
                .unwrap_or(0) as f64,
            avg_uncompressed_size: row_groups_stats
                .iter()
                .map(|rg| rg.uncompressed_size)
                .sum::<i64>() as f64
                / row_groups_stats.len() as f64,
            median_uncompressed_size: row_groups_stats
                .iter()
                .map(|rg| rg.uncompressed_size)
                .sorted()
                .nth(row_groups_stats.len() / 2)
                .unwrap_or(0) as f64,
            avg_rows_per_rg: row_groups_stats.iter().map(|rg| rg.rows).sum::<i64>() as f64
                / row_groups_stats.len() as f64,
            median_rows_per_rg: row_groups_stats
                .iter()
                .map(|rg| rg.rows)
                .sorted()
                .nth(row_groups_stats.len() / 2)
                .unwrap_or(0) as f64,
            avg_compression_ratio: row_groups_stats
                .iter()
                .map(|rg| rg.compression_ratio)
                .sum::<f64>()
                / row_groups_stats.len() as f64,
            median_compression_ratio: row_groups_stats
                .iter()
                .map(|rg| rg.compression_ratio)
                .sorted_by(|a, b| a.partial_cmp(b).unwrap())
                .nth(row_groups_stats.len() / 2)
                .unwrap_or(0.),
        }
    }
}

pub struct RowGroups {
    pub row_groups: Vec<RowGroupStats>,
    pub avg_median_stats: RowGroupAvgMedianStats,
}

impl RowGroups {
    pub fn from_file_reader<R: ChunkReader + 'static>(
        reader: &SerializedFileReader<R>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let row_groups = (0..reader.metadata().num_row_groups())
            .map(|idx| RowGroupStats::from_file_reader(reader, idx))
            .collect::<Result<Vec<_>, _>>()?;

        let avg_median_stats = RowGroupAvgMedianStats::new(&row_groups);

        Ok(Self {
            row_groups,
            avg_median_stats,
        })
    }

    pub fn num_row_groups(&self) -> usize {
        self.row_groups.len()
    }
}

pub struct RowGroupStats {
    pub idx: usize,
    pub rows: i64,
    pub compressed_size: i64,
    pub uncompressed_size: i64,
    pub compression_ratio: f64,
    pub column_metadata: Vec<RowGroupColumnMetadata>,
}

impl RowGroupStats {
    pub fn from_file_reader<R: ChunkReader + 'static>(
        reader: &SerializedFileReader<R>,
        idx: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let rg_md: &RowGroupMetaData = reader.metadata().row_group(idx);
        let compressed_size = rg_md.columns().iter().map(|c| c.compressed_size()).sum();
        let uncompressed_size = rg_md.columns().iter().map(|c| c.uncompressed_size()).sum();
        let compression_ratio = uncompressed_size as f64 / compressed_size as f64;

        let column_metadata = (0..rg_md.num_columns())
            .map(|col_idx| RowGroupColumnMetadata::from_file_reader(reader, idx, col_idx))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(RowGroupStats {
            idx,
            rows: rg_md.num_rows(),
            compressed_size,
            uncompressed_size,
            compression_ratio,
            column_metadata,
        })
    }
}

impl RowGroupColumnMetadata {
    pub fn from_file_reader<R: ChunkReader + 'static>(
        reader: &SerializedFileReader<R>,
        rg_idx: usize,
        col_idx: usize,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let rg_md = reader.metadata().row_group(rg_idx);
        let column_chunk: &ColumnChunkMetaData = rg_md.column(col_idx);

        let statistics = RowGroupColumnStats::new(column_chunk.statistics());

        Ok(RowGroupColumnMetadata {
            // file offset deprecated, show N/A if it is not valid
            file_offset: u64::try_from(column_chunk.file_offset())
                .ok()
                .filter(|&offset| offset > 0),
            column_path: column_chunk.column_descr().path().to_string(),
            has_stats: HasStats {
                has_stats: column_chunk.statistics().is_some(),
                has_dictionary_page: column_chunk.dictionary_page_offset().is_some(),
                has_bloom_filter: column_chunk.bloom_filter_offset().is_some(),
                has_page_encoding_stats: column_chunk.page_encoding_stats().is_some()
                    && !column_chunk.page_encoding_stats().unwrap().is_empty(),
            },
            statistics,
            total_compressed_size: column_chunk.compressed_size(),
            total_uncompressed_size: column_chunk.uncompressed_size(),
            compression_type: column_chunk.compression().to_string(),
        })
    }
}

/// Enumerate the pages of a single column chunk.
///
/// This reads (and decompresses) each page's data buffer, so it is intentionally
/// called lazily — only for the row group / column the user is currently viewing —
/// rather than eagerly for the whole file. See `ParquetCtx::page_info`.
pub(crate) fn make_page_info(page_reader: &mut Box<dyn PageReader>) -> RowGroupPageInfo {
    let mut page_info = Vec::new();
    while let Ok(Some(page)) = page_reader.get_next_page() {
        page_info.push(PageInfo::from(&page));
    }
    RowGroupPageInfo {
        page_infos: page_info,
    }
}

impl From<&Page> for PageInfo {
    fn from(page: &Page) -> Self {
        // Get the page reader for this column
        let page_type = match page.page_type() {
            PageType::DATA_PAGE => "Data Page".to_string(),
            PageType::INDEX_PAGE => "Index Page".to_string(),
            PageType::DICTIONARY_PAGE => "Dictionary Page".to_string(),
            PageType::DATA_PAGE_V2 => "Data Page V2".to_string(),
        };

        let encoding: String = match page.encoding() {
            Encoding::PLAIN => "Plain".to_string(),
            Encoding::PLAIN_DICTIONARY => "Plain Dictionary".to_string(),
            Encoding::RLE => "RLE".to_string(),
            Encoding::DELTA_BINARY_PACKED => "Delta Binary Packed".to_string(),
            Encoding::DELTA_LENGTH_BYTE_ARRAY => "Delta Length Byte Array".to_string(),
            Encoding::DELTA_BYTE_ARRAY => "Delta Byte Array".to_string(),
            Encoding::RLE_DICTIONARY => "RLE Dictionary".to_string(),
            Encoding::BYTE_STREAM_SPLIT => "Byte Stream Split".to_string(),
            _ => format!("{:?}", page.encoding()), // Handle any other encoding types
        };

        PageInfo {
            page_type,
            size: page.buffer().len(),
            rows: page.num_values() as usize,
            encoding,
        }
    }
}

macro_rules! extract_stat_value {
    ($stats:expr, $method:ident) => {
        match $stats {
            Statistics::Boolean(s) => s.$method().map(|v| v.to_string()),
            Statistics::Int32(s) => s.$method().map(|v| v.to_string()),
            Statistics::Int64(s) => s.$method().map(|v| v.to_string()),
            Statistics::Int96(s) => s.$method().map(|v| format!("{:?}", v)),
            Statistics::Float(s) => s.$method().map(|v| v.to_string()),
            Statistics::Double(s) => s.$method().map(|v| v.to_string()),
            Statistics::ByteArray(s) => s.$method().and_then(|bytes| {
                std::str::from_utf8(bytes.data())
                    .ok()
                    .map(|s| s.to_string())
            }),
            Statistics::FixedLenByteArray(s) => s.$method().and_then(|bytes| {
                std::str::from_utf8(bytes.data())
                    .ok()
                    .map(|s| s.to_string())
            }),
        }
    };
}

impl RowGroupColumnStats {
    fn new(stats: Option<&Statistics>) -> Option<Self> {
        stats.map(|stats| Self {
            min: extract_stat_value!(stats, min_opt),
            max: extract_stat_value!(stats, max_opt),
            null_count: stats.null_count_opt(),
            distinct_count: stats.distinct_count_opt(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_path(filename: &str) -> String {
        format!("{}/{}", crate::file::parquet_test_data(), filename)
    }

    fn open_reader(filename: &str) -> SerializedFileReader<std::fs::File> {
        let path = test_data_path(filename);
        let file = std::fs::File::open(&path).unwrap();
        SerializedFileReader::new(file).unwrap()
    }

    #[test]
    fn test_make_page_info_reads_dictionary_and_data_pages() {
        let reader = open_reader("alltypes_plain.parquet");
        let mut page_reader = reader
            .get_row_group(0)
            .unwrap()
            .get_column_page_reader(0)
            .unwrap();

        let pages = make_page_info(&mut page_reader);

        assert_eq!(pages.page_infos.len(), 2);
        assert_eq!(pages.page_infos[0].page_type, "Dictionary Page");
        assert_eq!(pages.page_infos[1].page_type, "Data Page");
        // The dictionary/plain files have a single row group of 8 rows.
        assert_eq!(pages.page_infos[1].rows, 8);
    }

    #[test]
    fn test_make_page_info_no_dictionary_page_for_bool_column() {
        let reader = open_reader("alltypes_plain.parquet");
        // Column 1 is the boolean column, which parquet never dictionary-encodes.
        let mut page_reader = reader
            .get_row_group(0)
            .unwrap()
            .get_column_page_reader(1)
            .unwrap();

        let pages = make_page_info(&mut page_reader);

        assert_eq!(pages.page_infos.len(), 1);
        assert_eq!(pages.page_infos[0].page_type, "Data Page");
    }

    #[test]
    fn test_row_group_page_info_default_is_empty() {
        let pages = RowGroupPageInfo::default();
        assert!(pages.page_infos.is_empty());
    }

    #[test]
    fn test_row_group_column_metadata_from_file_reader_succeeds_without_reading_pages() {
        // Regression test for the lazy page loading change: building column metadata
        // must not require (or fail because of) page enumeration.
        let reader = open_reader("alltypes_plain.parquet");
        let metadata = RowGroupColumnMetadata::from_file_reader(&reader, 0, 0).unwrap();

        assert_eq!(metadata.column_path, "\"id\"");
        assert!(metadata.has_stats.has_dictionary_page);
    }
}
