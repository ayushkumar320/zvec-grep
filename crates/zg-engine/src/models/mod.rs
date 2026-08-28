//! Private embedding model implementations matching the TypeScript engine.

mod catalog;
mod embedding;
mod error;
mod factory;
mod model2vec;
mod resolution;
pub(crate) mod runtime;

#[cfg(test)]
mod tests;
