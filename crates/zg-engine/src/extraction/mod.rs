//! Deterministic source extraction used by the indexing and lexical-enrichment paths.

mod code;
mod markdown;
mod text;

use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::{Content, ContentRange, EngineError, EntityMetadata, FileKind};

const HEX: &[u8; 16] = b"0123456789abcdef";

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

pub(crate) fn extract(
    source: &TextSource,
    options: ChunkOptions,
) -> Result<Vec<EntityFragment>, EngineError> {
    Ok(extract_for_indexing(source, options)?
        .into_iter()
        .map(|item| item.fragment)
        .collect())
}

pub(crate) fn extract_for_indexing(
    source: &TextSource,
    options: ChunkOptions,
) -> Result<Vec<IndexingExtractionFragment>, EngineError> {
    match source.file.kind {
        FileKind::Code => code::extract_for_indexing(source, options),
        _ if source.file.format == "markdown" => markdown::extract(source, options).map(|items| {
            items
                .into_iter()
                .map(|fragment| IndexingExtractionFragment {
                    fragment,
                    embedding_source: None,
                })
                .collect()
        }),
        _ => text::extract(source, options).map(|items| {
            items
                .into_iter()
                .map(|fragment| IndexingExtractionFragment {
                    fragment,
                    embedding_source: None,
                })
                .collect()
        }),
    }
}

pub(crate) fn vector_content_for_fragment(
    fragment: &EntityFragment,
    embedding_content: Option<&Content>,
    max_chars: Option<usize>,
) -> Content {
    let content = embedding_content.unwrap_or(&fragment.content);
    let Content::Text(text) = content else {
        return content.clone();
    };

    let metadata = vector_metadata_text(fragment.metadata.as_ref(), metadata_budget(max_chars));
    if metadata.is_empty() {
        return content.clone();
    }

    Content::Text(format!("{metadata}\n{text}"))
}

fn validate_source_file(source: &TextSource) -> Result<(), EngineError> {
    if source.file.id.trim().is_empty() {
        return Err(EngineError::invalid_input(
            "extractor source requires a non-empty file id",
        ));
    }
    if source
        .file
        .absolute_path
        .to_string_lossy()
        .trim()
        .is_empty()
    {
        return Err(EngineError::invalid_input(
            "extractor source requires a non-empty absolute file path",
        ));
    }
    if source
        .file
        .relative_path
        .to_string_lossy()
        .trim()
        .is_empty()
    {
        return Err(EngineError::invalid_input(
            "extractor source requires a non-empty relative file path",
        ));
    }
    Ok(())
}

fn make_entity_id(file_id: &str, index: usize) -> String {
    let digest = Sha256::digest(format!("{file_id}\0{index}").as_bytes());
    let mut id = String::with_capacity(64);
    for byte in digest {
        id.push(char::from(HEX[usize::from(byte >> 4)]));
        id.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    id
}

fn chunk_options_for_metadata(
    max_chunk_chars: usize,
    chunk_overlap_chars: usize,
    metadata: Option<&EntityMetadata>,
) -> (usize, usize) {
    let metadata_text = vector_metadata_text(metadata, metadata_budget(Some(max_chunk_chars)));
    let separator_chars = usize::from(!metadata_text.is_empty());
    let content_max = max_chunk_chars
        .saturating_sub(char_count(&metadata_text) + separator_chars)
        .max(1);
    let overlap = chunk_overlap_chars.min(content_max.saturating_sub(1));
    (content_max, overlap)
}

fn fit_text_to_chars(value: &str, max_chars: usize) -> String {
    if char_count(value) <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let prefix = take_chars(value, max_chars - 3).trim_end();
    format!("{prefix}...")
}

fn vector_metadata_text(metadata: Option<&EntityMetadata>, max_chars: Option<usize>) -> String {
    let Some(metadata) = metadata else {
        return String::new();
    };

    let lines = match metadata {
        EntityMetadata::Code {
            symbol_type,
            symbol_name,
            scope,
            signature,
            documentation,
            modifiers,
            ..
        } => vec![
            Some(match symbol_name {
                Some(name) => format!("symbol: {} {name}", symbol_type_name(*symbol_type)),
                None => format!("symbol: {}", symbol_type_name(*symbol_type)),
            }),
            scope.as_ref().map(|value| format!("scope: {value}")),
            signature
                .as_ref()
                .map(|value| format!("signature: {}", one_line(value))),
            (!modifiers.is_empty()).then(|| format!("modifiers: {}", modifiers.join(" "))),
            documentation
                .as_ref()
                .map(|value| format!("doc: {}", one_line(value))),
        ],
        EntityMetadata::Markdown {
            heading,
            level,
            scope,
        } => vec![
            heading.as_ref().map(|value| format!("heading: {value}")),
            level.map(|value| format!("heading_level: {value}")),
            scope.as_ref().map(|value| format!("scope: {value}")),
        ],
    };

    let text = lines.into_iter().flatten().collect::<Vec<_>>().join("\n");
    max_chars.map_or(text.clone(), |limit| fit_text_to_chars(&text, limit))
}

fn metadata_budget(max_chars: Option<usize>) -> Option<usize> {
    max_chars.map(|value| value / 4)
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn symbol_type_name(symbol_type: crate::SymbolType) -> &'static str {
    match symbol_type {
        crate::SymbolType::Module => "module",
        crate::SymbolType::Class => "class",
        crate::SymbolType::Interface => "interface",
        crate::SymbolType::Function => "function",
        crate::SymbolType::Value => "value",
        crate::SymbolType::Alias => "alias",
    }
}

fn char_count(value: &str) -> usize {
    value.encode_utf16().count()
}

fn take_chars(value: &str, count: usize) -> &str {
    let mut units = 0;
    for (index, character) in value.char_indices() {
        let next = units + character.len_utf16();
        if next > count {
            return &value[..index];
        }
        units = next;
    }
    value
}

fn byte_index_at_utf16(value: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (index, character) in value.char_indices() {
        let next = units + character.len_utf16();
        if next > utf16_offset {
            return index;
        }
        units = next;
    }
    value.len()
}

fn byte_index_at_utf16_ceil(value: &str, utf16_offset: usize) -> usize {
    let mut units = 0;
    for (index, character) in value.char_indices() {
        if units >= utf16_offset {
            return index;
        }
        units += character.len_utf16();
        if units > utf16_offset {
            return index + character.len_utf8();
        }
    }
    value.len()
}

#[cfg(test)]
fn test_source(kind: FileKind, format: &str, relative_path: &str, text: &str) -> TextSource {
    TextSource {
        file: SourceFile {
            id: format!("file-{format}"),
            absolute_path: PathBuf::from("/repo").join(relative_path),
            relative_path: PathBuf::from(relative_path),
            root_path: PathBuf::from("/repo"),
            size_bytes: text.len() as u64,
            modified_epoch_ms: Some(1),
            content_hash: None,
            kind,
            format: format.to_owned(),
        },
        text: text.to_owned(),
    }
}
