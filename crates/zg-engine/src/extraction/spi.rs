//! Types exchanged between extraction and the rest of the engine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    api::context::result::{ContentRange, EntityMetadata},
    payload::{Content, ImageFormat},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FileKind {
    Text,
    Code,
    Data,
    Image,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct TextRange {
    pub start_line: usize,
    pub end_line: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

impl From<TextRange> for ContentRange {
    fn from(range: TextRange) -> Self {
        Self::Text {
            start_line: range.start_line,
            end_line: range.end_line,
            start_offset: range.start_offset,
            end_offset: range.end_offset,
        }
    }
}

/// Source metadata required to produce stable extraction entities.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceFile {
    pub id: String,
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub root_path: PathBuf,
    pub size_bytes: u64,
    pub modified_epoch_ms: Option<u64>,
    pub content_hash: Option<String>,
    pub kind: FileKind,
    pub format: String,
}

/// UTF-8 text source prepared by the native scanner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TextSource {
    pub file: SourceFile,
    pub text: String,
}

/// Binary image source prepared by the native scanner.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ImageSource {
    pub file: SourceFile,
    pub data: Vec<u8>,
    pub format: ImageFormat,
}

/// Borrowed extraction input routed by source representation before file semantics.
pub(crate) enum Source<'source> {
    Text(&'source TextSource),
    Image(&'source ImageSource),
}

impl<'source> From<&'source TextSource> for Source<'source> {
    fn from(source: &'source TextSource) -> Self {
        Self::Text(source)
    }
}

impl<'source> From<&'source ImageSource> for Source<'source> {
    fn from(source: &'source ImageSource) -> Self {
        Self::Image(source)
    }
}

/// Per-call chunking limits and ranges use UTF-16 code-unit counts, matching
/// the TypeScript implementation and the public lexical range contract.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct ChunkOptions {
    pub max_chunk_chars: Option<usize>,
    pub chunk_overlap_chars: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EntityFragment {
    pub id: String,
    pub group: Option<String>,
    pub file_id: String,
    pub range: ContentRange,
    pub content: Content,
    pub metadata: Option<EntityMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct IndexingExtractionFragment {
    pub fragment: EntityFragment,
    /// Compacted source used only for embedding. Stored content always remains
    /// a byte-for-byte slice of the original source.
    pub embedding_source: Option<Content>,
}
