use parquet::file::reader::{FileReader, SerializedFileReader};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::rc::Rc;

use crate::file::error::FileIOError;
use crate::file::metadata::FileMetadata;
use crate::file::row_groups::{RowGroupPageInfo, RowGroups, make_page_info};
use crate::file::sample_data::ParquetSampleData;
use crate::file::schema::FileSchema;
pub struct ParquetCtx {
    pub file_path: String,
    pub metadata: FileMetadata,
    pub row_groups: RowGroups,
    pub schema: FileSchema,
    pub sample_data: ParquetSampleData,
    /// Retained so page info can be read lazily (reuses the already-parsed footer).
    reader: SerializedFileReader<File>,
    /// Caches page info per (row group, column) so it is read/decompressed at most once.
    page_cache: RefCell<HashMap<(usize, usize), Rc<RowGroupPageInfo>>>,
}

impl ParquetCtx {
    pub fn from_file(file_path: &str) -> Result<ParquetCtx, FileIOError> {
        let path = PathBuf::from(file_path);

        let file = File::open(&path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => FileIOError::FileNotFound { path: path.clone() },
            std::io::ErrorKind::PermissionDenied => {
                FileIOError::PermissionDenied { path: path.clone() }
            }
            _ => FileIOError::Io { source: e },
        })?;

        let reader: SerializedFileReader<File> =
            SerializedFileReader::new(file).map_err(|e| FileIOError::InvalidParquet {
                path: path.clone(),
                details: e.to_string(),
            })?;

        let md = reader.metadata();

        let row_groups =
            RowGroups::from_file_reader(&reader).map_err(|e| FileIOError::MetadataError {
                details: format!("Failed to read row groups: {e}"),
            })?;

        let metadata = FileMetadata::from_metadata(md).map_err(|e| FileIOError::MetadataError {
            details: format!("Failed to read file metadata: {e}"),
        })?;

        let schema = FileSchema::from_metadata(md).map_err(|e| FileIOError::MetadataError {
            details: format!("Failed to parse schema: {e}"),
        })?;

        let sample_data = ParquetSampleData::open(file_path, metadata.num_rows).map_err(|e| {
            FileIOError::SampleDataError {
                details: e.to_string(),
            }
        })?;

        Ok(ParquetCtx {
            file_path: file_path.to_string(),
            metadata,
            row_groups,
            schema,
            sample_data,
            reader,
            page_cache: RefCell::new(HashMap::new()),
        })
    }

    pub fn column_size(&self) -> usize {
        self.schema.column_size()
    }

    /// Lazily read (and cache) the page info for a single column chunk.
    ///
    /// Page enumeration decompresses each page's data buffer, so this is done on
    /// demand only for the row group / column currently being viewed. Results are
    /// cached, so re-navigating to the same column is free.
    pub fn page_info(&self, rg_idx: usize, col_idx: usize) -> Rc<RowGroupPageInfo> {
        if let Some(pages) = self.page_cache.borrow().get(&(rg_idx, col_idx)) {
            return Rc::clone(pages);
        }

        let pages = Rc::new(self.read_page_info(rg_idx, col_idx).unwrap_or_default());
        self.page_cache
            .borrow_mut()
            .insert((rg_idx, col_idx), Rc::clone(&pages));
        pages
    }

    fn read_page_info(
        &self,
        rg_idx: usize,
        col_idx: usize,
    ) -> Result<RowGroupPageInfo, Box<dyn std::error::Error>> {
        let mut page_reader = self
            .reader
            .get_row_group(rg_idx)?
            .get_column_page_reader(col_idx)?;
        Ok(make_page_info(&mut page_reader))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data_path(filename: &str) -> String {
        format!("{}/{}", crate::file::parquet_test_data(), filename)
    }

    #[test]
    fn test_file_not_found() {
        let path = test_data_path("this_file_does_not_exist.parquet");
        let Err(err) = ParquetCtx::from_file(&path) else {
            panic!("Expected error for nonexistent file");
        };
        assert!(
            matches!(err, FileIOError::FileNotFound { .. }),
            "Expected FileNotFound, got: {err}"
        );
    }

    #[test]
    fn test_non_parquet_file() {
        let path = test_data_path("README.md");
        let Err(err) = ParquetCtx::from_file(&path) else {
            panic!("Expected error for non-parquet file");
        };
        assert!(
            matches!(err, FileIOError::InvalidParquet { .. }),
            "Expected InvalidParquet, got: {err}"
        );
    }

    #[test]
    fn test_valid_parquet_file_alltypes_plain() {
        let path = test_data_path("alltypes_plain.parquet");
        let result = ParquetCtx::from_file(&path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn test_valid_parquet_file_alltypes_dictionary() {
        let path = test_data_path("alltypes_dictionary.parquet");
        let result = ParquetCtx::from_file(&path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn test_valid_parquet_file_nulls() {
        let path = test_data_path("nulls.snappy.parquet");
        let result = ParquetCtx::from_file(&path);
        assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn test_file_not_found_error_contains_path() {
        let path = test_data_path("nonexistent.parquet");
        let Err(err) = ParquetCtx::from_file(&path) else {
            panic!("Expected error for nonexistent file");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("nonexistent.parquet"),
            "Error message should contain the file path, got: {msg}"
        );
    }

    #[test]
    fn test_invalid_parquet_error_contains_path() {
        let path = test_data_path("README.md");
        let Err(err) = ParquetCtx::from_file(&path) else {
            panic!("Expected error for non-parquet file");
        };
        let msg = err.to_string();
        assert!(
            msg.contains("README.md"),
            "Error message should contain the file path, got: {msg}"
        );
    }

    #[test]
    fn test_corrupt_parquet_from_bad_data() {
        let path = format!(
            "{}/../../parquet-testing/bad_data/PARQUET-1481.parquet",
            crate::file::parquet_test_data()
        );
        let result = ParquetCtx::from_file(&path);
        assert!(result.is_err(), "Expected error for corrupt parquet file");
    }

    #[test]
    fn test_page_info_reads_pages_for_column() {
        let path = test_data_path("alltypes_plain.parquet");
        let ctx = ParquetCtx::from_file(&path).unwrap();

        let pages = ctx.page_info(0, 0);

        assert_eq!(pages.page_infos.len(), 2);
        assert_eq!(pages.page_infos[0].page_type, "Dictionary Page");
        assert_eq!(pages.page_infos[1].page_type, "Data Page");
    }

    #[test]
    fn test_page_info_differs_per_column() {
        let path = test_data_path("alltypes_plain.parquet");
        let ctx = ParquetCtx::from_file(&path).unwrap();

        // Column 1 (bool) has no dictionary page, unlike column 0.
        let pages = ctx.page_info(0, 1);

        assert_eq!(pages.page_infos.len(), 1);
        assert_eq!(pages.page_infos[0].page_type, "Data Page");
    }

    #[test]
    fn test_page_info_handles_many_pages() {
        let path = test_data_path("alltypes_tiny_pages.parquet");
        let ctx = ParquetCtx::from_file(&path).unwrap();

        let pages = ctx.page_info(0, 0);

        assert_eq!(pages.page_infos.len(), 325);
    }

    #[test]
    fn test_page_info_caches_repeated_lookups() {
        let path = test_data_path("alltypes_plain.parquet");
        let ctx = ParquetCtx::from_file(&path).unwrap();

        let first = ctx.page_info(0, 0);
        let second = ctx.page_info(0, 0);

        assert!(
            Rc::ptr_eq(&first, &second),
            "expected cached page info to be reused rather than re-read"
        );
        assert_eq!(ctx.page_cache.borrow().len(), 1);
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_page_info_out_of_range_row_group_panics() {
        // `unwrap_or_default()` in `page_info` only catches `Err`s from
        // `read_page_info`, but the underlying `parquet` crate indexes into its
        // row group vector directly and panics on an out-of-range index rather
        // than returning `Err`. Documenting this so a caller doesn't assume
        // `page_info` is panic-safe for arbitrary indices.
        let path = test_data_path("alltypes_plain.parquet");
        let ctx = ParquetCtx::from_file(&path).unwrap();

        ctx.page_info(99, 99);
    }
}
