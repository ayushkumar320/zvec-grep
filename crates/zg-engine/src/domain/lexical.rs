//! Lexical search requests and replies.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::TextRange;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct LexicalSearchRequest {
    /// Workspace root. `None` uses the process working directory.
    pub root: Option<PathBuf>,
    pub patterns: Vec<String>,
    pub pattern_files: Vec<PathBuf>,
    pub paths: Vec<PathBuf>,
    pub limit: Option<usize>,
    pub options: LexicalOptions,
}

#[derive(Debug, Error)]
pub enum ManagedRgArgumentError {
    #[error("zg query --rg requires a pattern")]
    MissingPattern,
    #[error("unsupported --rg option in the POC: {0}")]
    UnsupportedOption(String),
    #[error("{option} requires a value")]
    MissingOptionValue { option: String },
    #[error("invalid value {value:?} for {option}")]
    InvalidOptionValue { option: String, value: String },
}

/// Parses the managed-ripgrep argument dialect shared by CLI and MCP.
///
/// # Errors
///
/// Returns [`ManagedRgArgumentError`] for a missing pattern, unsupported
/// option, missing option value, or invalid numeric value.
pub fn parse_managed_rg_args(
    args: &[String],
) -> Result<LexicalSearchRequest, ManagedRgArgumentError> {
    let mut request = LexicalSearchRequest::default();
    let mut index = 0;
    let mut options_finished = false;
    let mut positionals = Vec::new();

    while index < args.len() {
        let arg = &args[index];
        if options_finished {
            positionals.push(arg.clone());
            index += 1;
            continue;
        }
        if arg == "--" {
            options_finished = true;
            index += 1;
            continue;
        }

        match arg.as_str() {
            "-n" | "--line-number" => {}
            "-F" | "--fixed-strings" => request.options.fixed_strings = true,
            "-i" | "--ignore-case" => request.options.ignore_case = true,
            "-w" | "--word-regexp" => request.options.word_regexp = true,
            "--hidden" => request.options.hidden = true,
            "--no-ignore" => request.options.no_ignore = true,
            "--follow" => request.options.follow = true,
            "-g" | "--glob" => {
                request
                    .options
                    .globs
                    .push(take_value(args, &mut index, arg)?);
            }
            "-t" | "--type" => {
                request
                    .options
                    .file_types
                    .push(take_value(args, &mut index, arg)?);
            }
            "-T" | "--type-not" => {
                request
                    .options
                    .excluded_file_types
                    .push(take_value(args, &mut index, arg)?);
            }
            "--ignore-file" => request
                .options
                .ignore_files
                .push(PathBuf::from(take_value(args, &mut index, arg)?)),
            "--max-depth" => {
                request.options.max_depth = Some(take_usize(args, &mut index, arg)?);
            }
            "--max-filesize" => {
                request.options.max_file_size_bytes = Some(take_u64(args, &mut index, arg)?);
            }
            "-A" | "--after-context" => {
                request.options.after_context = take_usize(args, &mut index, arg)?;
            }
            "-B" | "--before-context" => {
                request.options.before_context = take_usize(args, &mut index, arg)?;
            }
            "-C" | "--context" => {
                let value = take_usize(args, &mut index, arg)?;
                request.options.before_context = value;
                request.options.after_context = value;
            }
            "-e" | "--regexp" => {
                request.patterns.push(take_value(args, &mut index, arg)?);
            }
            "-f" | "--file" => request
                .pattern_files
                .push(PathBuf::from(take_value(args, &mut index, arg)?)),
            value if value.starts_with('-') => {
                return Err(ManagedRgArgumentError::UnsupportedOption(value.to_owned()));
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }

    if request.patterns.is_empty() && request.pattern_files.is_empty() {
        if positionals.is_empty() {
            return Err(ManagedRgArgumentError::MissingPattern);
        }
        request.patterns.push(positionals.remove(0));
    }
    request.paths = positionals.into_iter().map(PathBuf::from).collect();
    Ok(request)
}

fn take_value(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, ManagedRgArgumentError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| ManagedRgArgumentError::MissingOptionValue {
            option: option.to_owned(),
        })
}

fn take_usize(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<usize, ManagedRgArgumentError> {
    let value = take_value(args, index, option)?;
    value
        .parse()
        .map_err(|_| ManagedRgArgumentError::InvalidOptionValue {
            option: option.to_owned(),
            value,
        })
}

fn take_u64(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<u64, ManagedRgArgumentError> {
    let value = take_value(args, index, option)?;
    value
        .parse()
        .map_err(|_| ManagedRgArgumentError::InvalidOptionValue {
            option: option.to_owned(),
            value,
        })
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct LexicalOptions {
    pub fixed_strings: bool,
    pub ignore_case: bool,
    pub word_regexp: bool,
    pub before_context: usize,
    pub after_context: usize,
    pub hidden: bool,
    pub no_ignore: bool,
    pub follow: bool,
    pub globs: Vec<String>,
    pub file_types: Vec<String>,
    pub excluded_file_types: Vec<String>,
    pub ignore_files: Vec<PathBuf>,
    pub max_depth: Option<usize>,
    pub max_file_size_bytes: Option<u64>,
    pub modified_after_epoch_ms: Option<u64>,
    pub modified_before_epoch_ms: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LexicalMatch {
    pub rank: usize,
    pub absolute_path: PathBuf,
    pub relative_path: PathBuf,
    pub range: TextRange,
    pub excerpt_range: Option<TextRange>,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LexicalDiagnostics {
    pub backend: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub ignored_directories: Vec<PathBuf>,
    pub missing_paths: Vec<PathBuf>,
    pub searched_paths: Vec<PathBuf>,
    pub limit: Option<usize>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LexicalSearchReply {
    pub root: PathBuf,
    pub coverage: LexicalCoverage,
    pub matches: Vec<LexicalMatch>,
    pub diagnostics: LexicalDiagnostics,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LexicalCoverage {
    Exhaustive,
    Truncated,
}
