//! Content payloads shared internally by extraction and embedding models.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub(crate) enum Content {
    Text(String),
    Image(ImageContent),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ImageContent {
    pub data: Vec<u8>,
    pub format: ImageFormat,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
}
