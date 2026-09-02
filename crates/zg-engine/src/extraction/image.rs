use crate::{
    EngineError,
    api::context::result::ContentRange,
    payload::{Content, ImageContent},
};

use super::{EntityFragment, ImageSource, make_entity_id, validate_source_file};

pub(super) fn extract(source: &ImageSource) -> Result<Vec<EntityFragment>, EngineError> {
    validate_source_file(&source.file)?;
    if source.data.is_empty() {
        return Err(EngineError::invalid_argument(
            "image extractor requires non-empty image data",
        ));
    }

    Ok(vec![EntityFragment {
        id: make_entity_id(&source.file.id, 0),
        group: None,
        file_id: source.file.id.clone(),
        range: ContentRange::File,
        content: Content::Image(ImageContent {
            data: source.data.clone(),
            format: source.format,
        }),
        metadata: None,
    }])
}

#[cfg(test)]
mod tests {
    use crate::{
        api::context::result::ContentRange,
        payload::{Content, ImageContent, ImageFormat},
    };

    use super::super::FileKind;

    use super::super::{
        ChunkOptions, ImageSource, extract as extract_source, extract_for_indexing, test_file,
    };
    use super::extract;

    fn image_source(data: Vec<u8>) -> ImageSource {
        ImageSource {
            file: test_file(FileKind::Image, "png", "fixture.png", data.len() as u64),
            data,
            format: ImageFormat::Png,
        }
    }

    #[test]
    fn preserves_image_bytes_format_and_file_range() {
        let source = image_source(vec![1, 2, 3]);
        let fragments = extract(&source).expect("image extraction");
        assert_eq!(fragments.len(), 1);
        assert_eq!(fragments[0].id.len(), 64);
        assert_eq!(fragments[0].file_id, source.file.id);
        assert_eq!(fragments[0].range, ContentRange::File);
        assert_eq!(fragments[0].group, None);
        assert_eq!(fragments[0].metadata, None);
        assert_eq!(
            fragments[0].content,
            Content::Image(ImageContent {
                data: vec![1, 2, 3],
                format: ImageFormat::Png,
            })
        );
    }

    #[test]
    fn rejects_empty_data_and_invalid_source_metadata() {
        assert!(extract(&image_source(Vec::new())).is_err());

        let mut missing_id = image_source(vec![1]);
        missing_id.file.id.clear();
        assert!(extract(&missing_id).is_err());

        let mut missing_absolute_path = image_source(vec![1]);
        missing_absolute_path.file.absolute_path.clear();
        assert!(extract(&missing_absolute_path).is_err());

        let mut missing_relative_path = image_source(vec![1]);
        missing_relative_path.file.relative_path.clear();
        assert!(extract(&missing_relative_path).is_err());
    }

    #[test]
    fn source_router_and_indexing_preserve_the_image_fragment() {
        let source = image_source(vec![4, 5, 6]);
        let direct = extract_source(&source, ChunkOptions::default()).expect("direct extraction");
        let indexing =
            extract_for_indexing(&source, ChunkOptions::default()).expect("indexing extraction");

        assert_eq!(indexing.len(), 1);
        assert_eq!(direct, vec![indexing[0].fragment.clone()]);
        assert_eq!(indexing[0].embedding_source, None);
    }
}
