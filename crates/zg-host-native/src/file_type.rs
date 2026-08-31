use std::path::Path;

use crate::FileKind;

pub(crate) const DEFAULT_MAX_CODE_FILE_SIZE_BYTES: u64 = 1024 * 1024;
pub(crate) const DEFAULT_MAX_TEXT_FILE_SIZE_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_DATA_FILE_SIZE_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_IMAGE_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DetectedFileType {
    pub kind: FileKind,
    pub format: String,
}

pub(crate) fn detect_file_type(path: &Path) -> Option<DetectedFileType> {
    let name = path.file_name()?.to_string_lossy();
    match name.as_ref() {
        "Dockerfile" => return Some(file_type(FileKind::Code, "dockerfile")),
        "Makefile" => return Some(file_type(FileKind::Code, "makefile")),
        _ => {}
    }

    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let extension = extension.as_str();

    let detected = match extension {
        "c" => file_type(FileKind::Code, "c"),
        "cc" | "cpp" | "cxx" | "h" | "hpp" => file_type(FileKind::Code, "cpp"),
        "go" => file_type(FileKind::Code, "go"),
        "java" => file_type(FileKind::Code, "java"),
        "js" | "mjs" | "cjs" => file_type(FileKind::Code, "javascript"),
        "jsx" => file_type(FileKind::Code, "jsx"),
        "ts" => file_type(FileKind::Code, "typescript"),
        "tsx" => file_type(FileKind::Code, "tsx"),
        "py" => file_type(FileKind::Code, "python"),
        "rs" => file_type(FileKind::Code, "rust"),
        "rb" => file_type(FileKind::Code, "ruby"),
        "php" => file_type(FileKind::Code, "php"),
        "swift" => file_type(FileKind::Code, "swift"),
        "kt" | "kts" => file_type(FileKind::Code, "kotlin"),
        "cs" => file_type(FileKind::Code, "csharp"),
        "scala" => file_type(FileKind::Code, "scala"),
        "sh" | "bash" | "zsh" => file_type(FileKind::Code, "bash"),
        "sql" => file_type(FileKind::Code, "sql"),
        "css" => file_type(FileKind::Code, "css"),
        "scss" => file_type(FileKind::Code, "scss"),
        "less" => file_type(FileKind::Code, "less"),
        "vue" => file_type(FileKind::Code, "vue"),
        "svelte" => file_type(FileKind::Code, "svelte"),
        "csv" => file_type(FileKind::Data, "csv"),
        "json" | "jsonc" => file_type(FileKind::Data, "json"),
        "toml" => file_type(FileKind::Data, "toml"),
        "yaml" | "yml" => file_type(FileKind::Data, "yaml"),
        "md" | "mdx" => file_type(FileKind::Text, "markdown"),
        "rst" => file_type(FileKind::Text, "rst"),
        "txt" => file_type(FileKind::Text, "text"),
        "html" | "htm" => file_type(FileKind::Text, "html"),
        "xml" => file_type(FileKind::Text, "xml"),
        "gif" => file_type(FileKind::Image, "gif"),
        "jpeg" | "jpg" => file_type(FileKind::Image, "jpeg"),
        "png" => file_type(FileKind::Image, "png"),
        "webp" => file_type(FileKind::Image, "webp"),
        extension if is_known_binary_extension(extension) => return None,
        "" => file_type(FileKind::Text, "text"),
        extension => file_type(FileKind::Text, extension),
    };
    Some(detected)
}

pub(crate) fn max_file_size(kind: FileKind, explicit: Option<u64>) -> u64 {
    explicit.unwrap_or(match kind {
        FileKind::Code => DEFAULT_MAX_CODE_FILE_SIZE_BYTES,
        FileKind::Text => DEFAULT_MAX_TEXT_FILE_SIZE_BYTES,
        FileKind::Data => DEFAULT_MAX_DATA_FILE_SIZE_BYTES,
        FileKind::Image => DEFAULT_MAX_IMAGE_FILE_SIZE_BYTES,
    })
}

fn file_type(kind: FileKind, format: &str) -> DetectedFileType {
    DetectedFileType {
        kind,
        format: format.to_owned(),
    }
}

fn is_known_binary_extension(extension: &str) -> bool {
    matches!(
        extension,
        "zip"
            | "tar"
            | "gz"
            | "bz2"
            | "xz"
            | "7z"
            | "rar"
            | "exe"
            | "dll"
            | "dylib"
            | "so"
            | "a"
            | "o"
            | "obj"
            | "wasm"
            | "class"
            | "jar"
            | "pdf"
            | "doc"
            | "docx"
            | "ppt"
            | "pptx"
            | "xls"
            | "xlsx"
            | "mp3"
            | "mp4"
            | "mov"
            | "avi"
            | "mkv"
            | "db"
            | "sqlite"
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{FileKind, detect_file_type, max_file_size};

    #[test]
    fn matches_typescript_file_detection_and_size_defaults() {
        let dockerfile = detect_file_type(Path::new("Dockerfile")).expect("named type");
        assert_eq!(dockerfile.kind, FileKind::Code);
        assert_eq!(dockerfile.format, "dockerfile");
        assert_eq!(
            detect_file_type(Path::new("README.mdx"))
                .expect("markdown")
                .format,
            "markdown"
        );
        assert_eq!(
            detect_file_type(Path::new("custom.xyz"))
                .expect("unknown text")
                .format,
            "xyz"
        );
        assert!(detect_file_type(Path::new("archive.zip")).is_none());
        assert_eq!(max_file_size(FileKind::Code, None), 1024 * 1024);
        assert_eq!(max_file_size(FileKind::Text, Some(42)), 42);
    }
}
