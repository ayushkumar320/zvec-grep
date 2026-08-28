//! Private embedding model implementations matching the TypeScript engine.

mod catalog;
mod compute;
mod download_progress;
mod embedding;
mod error;
mod factory;
mod llama_cpp;
mod model2vec;
mod qwen;
mod resolution;
pub(crate) mod runtime;
mod transformers;

#[cfg(test)]
mod tests;
