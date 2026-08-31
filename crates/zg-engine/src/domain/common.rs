//! Shared public value types used by the high-level requests and replies.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum Content {
    Text(String),
    Image(ImageContent),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImageContent {
    pub data: Vec<u8>,
    pub format: ImageFormat,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ContentRange {
    File,
    Text {
        start_line: usize,
        end_line: usize,
        start_offset: usize,
        end_offset: usize,
    },
    Byte {
        start_offset: u64,
        end_offset: u64,
    },
    Page {
        page: usize,
    },
    PageText {
        page: usize,
        start_offset: usize,
        end_offset: usize,
    },
    PageRegion {
        page: usize,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TextRange {
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

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Text,
    Code,
    Data,
    Image,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolType {
    Module,
    Class,
    Interface,
    Function,
    Value,
    Alias,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EntityMetadata {
    Code {
        symbol_type: SymbolType,
        symbol_name: Option<String>,
        scope: Option<String>,
        node_type: Option<String>,
        signature: Option<String>,
        documentation: Option<String>,
        modifiers: Vec<String>,
    },
    Markdown {
        heading: Option<String>,
        level: Option<usize>,
        scope: Option<String>,
    },
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct DiscoveryOptions {
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub globs: Vec<String>,
    pub insensitive_globs: Vec<String>,
    pub file_types: Vec<String>,
    pub excluded_file_types: Vec<String>,
    pub hidden: bool,
    pub no_ignore: bool,
    pub ignore_files: Vec<PathBuf>,
    pub max_depth: Option<usize>,
    pub max_file_size_bytes: Option<u64>,
    pub follow: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RootSpec {
    pub path: PathBuf,
    pub recursive: bool,
    pub discovery: DiscoveryOptions,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Device {
    Auto,
    Cpu,
    Metal,
    Vulkan,
    Cuda,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingMetric {
    Cosine,
    DotProduct,
    Euclidean,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingInputKind {
    Text,
    Image,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EmbeddingModelSpec {
    pub reference: String,
    pub revision: Option<String>,
    pub cache_dir: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub device: Device,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimingEntry {
    pub name: String,
    pub duration_micros: u64,
    pub count: Option<u64>,
}
