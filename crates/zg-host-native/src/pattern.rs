use std::path::Path;

use globset::{GlobBuilder, GlobMatcher};
use zg_engine::CoreError;

#[derive(Clone, Debug)]
pub(crate) struct PathPattern {
    normalized: String,
    matcher: Option<GlobMatcher>,
    case_insensitive: bool,
}

impl PathPattern {
    pub fn path(pattern: &str) -> Result<Self, CoreError> {
        Self::build(pattern, false, false)
    }

    pub fn rg(pattern: &str, case_insensitive: bool) -> Result<Self, CoreError> {
        Self::build(pattern, case_insensitive, true)
    }

    fn build(pattern: &str, case_insensitive: bool, rg_semantics: bool) -> Result<Self, CoreError> {
        let normalized = normalize_path_pattern(pattern);
        if normalized.is_empty() {
            return Err(CoreError::invalid_input("glob pattern must not be empty"));
        }
        let matcher = if rg_semantics || has_path_glob(&normalized) {
            Some(compile_matcher(&normalized, case_insensitive)?)
        } else {
            None
        };
        Ok(Self {
            normalized,
            matcher,
            case_insensitive,
        })
    }

    pub fn is_match(&self, path: &str) -> bool {
        let path = normalize_path_for_match(path);
        if let Some(matcher) = &self.matcher {
            return matcher.is_match(&path) || self.matches_directory_itself(&path);
        }

        let (candidate, expected) = if self.case_insensitive {
            (path.to_lowercase(), self.normalized.to_lowercase())
        } else {
            (path, self.normalized.clone())
        };
        let prefix = if expected.ends_with('/') {
            expected.clone()
        } else {
            format!("{expected}/")
        };
        candidate == expected || candidate.starts_with(&prefix)
    }

    pub fn might_match_descendant(&self, directory_path: &str) -> bool {
        let directory = normalize_path_for_match(directory_path)
            .trim_end_matches('/')
            .to_owned();
        if directory.is_empty() {
            return true;
        }
        if self.is_match(&directory)
            || self.is_match(&format!("{directory}/__zvec_grep_descendant__"))
        {
            return true;
        }
        pattern_prefix_might_match_descendant(&self.normalized, &directory)
    }

    pub fn normalized(&self) -> &str {
        &self.normalized
    }

    fn matches_directory_itself(&self, path: &str) -> bool {
        let Some(directory_pattern) = self.normalized.strip_suffix("/**") else {
            return false;
        };
        compile_matcher(directory_pattern, self.case_insensitive)
            .is_ok_and(|matcher| matcher.is_match(path))
    }
}

pub(crate) fn normalize_path_pattern(pattern: &str) -> String {
    let mut normalized = collapse_slashes(&pattern.trim().replace('\\', "/"));
    if !is_absolute_path_pattern(&normalized) {
        while let Some(value) = normalized.strip_prefix("./") {
            normalized = value.to_owned();
        }
    }
    normalized
}

pub(crate) fn normalize_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

fn compile_matcher(pattern: &str, case_insensitive: bool) -> Result<GlobMatcher, CoreError> {
    let compiled_pattern = if !pattern.contains('/') && pattern != "**" {
        format!("**/{pattern}")
    } else {
        pattern.to_owned()
    };
    let mut builder = GlobBuilder::new(&compiled_pattern);
    builder
        .case_insensitive(case_insensitive)
        .literal_separator(true)
        .backslash_escape(false)
        .allow_unclosed_class(true);
    builder
        .build()
        .map(|glob| glob.compile_matcher())
        .map_err(|error| CoreError::invalid_input(format!("invalid glob {pattern:?}: {error}")))
}

fn normalize_path_for_match(path: &str) -> String {
    collapse_slashes(&path.replace('\\', "/"))
}

fn collapse_slashes(value: &str) -> String {
    let mut collapsed = String::with_capacity(value.len());
    let mut previous_slash = false;
    for character in value.chars() {
        if character == '/' {
            if !previous_slash {
                collapsed.push(character);
            }
            previous_slash = true;
        } else {
            collapsed.push(character);
            previous_slash = false;
        }
    }
    collapsed
}

fn is_absolute_path_pattern(pattern: &str) -> bool {
    pattern.starts_with('/')
        || pattern.as_bytes().get(1) == Some(&b':')
            && pattern.as_bytes().get(2) == Some(&b'/')
            && pattern
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphabetic)
}

fn has_path_glob(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn pattern_prefix_might_match_descendant(pattern: &str, directory: &str) -> bool {
    let directory_prefix = format!("{directory}/");
    let variants = pattern
        .strip_prefix("**/")
        .map_or_else(|| vec![pattern], |stripped| vec![pattern, stripped]);
    variants.into_iter().any(|variant| {
        if !has_path_glob(variant) {
            return variant.starts_with(&directory_prefix);
        }
        let literal_prefix = literal_prefix_before_first_glob(variant);
        !literal_prefix.is_empty()
            && (literal_prefix.starts_with(&directory_prefix)
                || directory_prefix.starts_with(literal_prefix))
    })
}

fn literal_prefix_before_first_glob(pattern: &str) -> &str {
    let first = pattern
        .char_indices()
        .find_map(|(index, character)| matches!(character, '*' | '?').then_some(index))
        .unwrap_or(pattern.len());
    &pattern[..first]
}

#[cfg(test)]
mod tests {
    use super::PathPattern;

    #[test]
    fn matches_typescript_path_and_ripgrep_glob_semantics() {
        let literal = PathPattern::path("src").expect("literal path pattern");
        assert!(literal.is_match("src"));
        assert!(literal.is_match("src/lib.rs"));
        assert!(!literal.is_match("nested/src/lib.rs"));

        let nested = PathPattern::path("**/*.test.ts").expect("nested path pattern");
        assert!(nested.is_match("main.test.ts"));
        assert!(nested.is_match("src/main.test.ts"));

        let rg = PathPattern::rg("*.rs", false).expect("rg glob");
        assert!(rg.is_match("lib.rs"));
        assert!(rg.is_match("src/lib.rs"));

        let insensitive = PathPattern::rg("README.*", true).expect("case insensitive glob");
        assert!(insensitive.is_match("docs/readme.MD"));
    }

    #[test]
    fn detects_possible_include_descendants() {
        let include = PathPattern::path(".vscode/settings.json").expect("include pattern");
        assert!(include.might_match_descendant(".vscode"));
        assert!(!include.might_match_descendant(".idea"));
    }
}
