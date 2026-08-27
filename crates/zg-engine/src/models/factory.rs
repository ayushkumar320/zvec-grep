use std::sync::Arc;

use super::{
    catalog::get_embedding_model_catalog_entry,
    embedding::{CreateEmbeddingModelOptions, EmbeddingModel},
    error::ModelError,
    model2vec::Model2VecEmbeddingModel,
};

/// Creates a catalog-backed embedding model.
///
/// `None` for `options` is the Rust equivalent of omitting the optional
/// TypeScript options object.
///
/// # Errors
///
/// Returns the TypeScript catalog-not-found error for unknown references. A
/// known backend not yet ported to Rust returns the stable model-not-implemented
/// error.
pub fn create_embedding_model(
    reference: &str,
    options: Option<CreateEmbeddingModelOptions>,
) -> Result<Arc<dyn EmbeddingModel>, ModelError> {
    let entry = get_embedding_model_catalog_entry(reference).ok_or_else(|| {
        ModelError::coded(
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_CATALOG_MODEL_NOT_FOUND",
            "Embedding model is not in the zvec-grep catalog",
            Some(format!("embedding={reference}")),
        )
    })?;
    let Some(config) = entry.model2vec_config() else {
        return Err(ModelError::coded(
            "ZVEC_GREP.ENGINE.MODELS.EMBEDDING_MODEL_NOT_IMPLEMENTED",
            "Embedding catalog entry is not implemented",
            Some(format!("backend={} reference={reference}", entry.backend())),
        ));
    };
    Ok(Arc::new(Model2VecEmbeddingModel::new(
        config,
        options.unwrap_or_default(),
    )))
}

#[cfg(test)]
mod tests {
    use super::create_embedding_model;

    #[test]
    fn factory_exposes_only_implemented_model2vec_backends() {
        let model = create_embedding_model("local/potion-code-16m-v2", None)
            .expect("Model2Vec backend should be implemented");
        assert_eq!(model.info().reference, "local/potion-code-16m-v2");

        let unimplemented = create_embedding_model("local/embeddinggemma-300m", None)
            .err()
            .expect("unported catalog backend should fail clearly");
        assert_eq!(
            unimplemented.code(),
            Some("ZVEC_GREP.ENGINE.MODELS.EMBEDDING_MODEL_NOT_IMPLEMENTED")
        );

        let unknown = create_embedding_model("missing", None)
            .err()
            .expect("unknown model should fail catalog lookup");
        assert_eq!(
            unknown.code(),
            Some("ZVEC_GREP.ENGINE.MODELS.EMBEDDING_CATALOG_MODEL_NOT_FOUND")
        );
    }
}
