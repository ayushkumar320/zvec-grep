use std::path::PathBuf;

use thiserror::Error;
use zg_engine::api::context::ContextOptions;

use crate::parse_byte_size;

#[derive(Debug, Error)]
pub enum ManagedRgArgumentError {
    #[error("zg query --rg requires a pattern")]
    MissingPattern,
    #[error("unsupported --rg option: {0}")]
    UnsupportedOption(String),
    #[error("{0} changes rg output and cannot be used with managed --rg")]
    OutputOption(String),
    #[error("{option} requires a value")]
    MissingOptionValue { option: String },
    #[error("invalid value {value:?} for {option}")]
    InvalidOptionValue { option: String, value: String },
}

/// Parses the safe managed-ripgrep argument dialect used by the CLI.
///
/// # Errors
///
/// Returns [`ManagedRgArgumentError`] for missing patterns, invalid values,
/// unsupported options, or options that replace managed output formatting.
pub fn parse_managed_rg_args(args: &[String]) -> Result<ContextOptions, ManagedRgArgumentError> {
    let mut request = ContextOptions {
        rg: true,
        ..ContextOptions::default()
    };
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
        if let Some((name, value)) = arg.split_once('=')
            && parse_long_with_value(name, value, &mut request)?
        {
            index += 1;
            continue;
        }
        match arg.as_str() {
            "-n" | "-H" | "--line-number" | "--with-filename" | "--recursive" => {}
            "-F" | "--fixed-strings" => {
                request.rg_options.fixed_strings = true;
            }
            "-i" | "--ignore-case" => {
                request.rg_options.ignore_case = true;
            }
            "-w" | "--word-regexp" => {
                request.rg_options.word_regexp = true;
            }
            "--hidden" => request.hidden = true,
            "--no-ignore" => request.no_ignore = true,
            "-L" | "--follow" => request.follow = true,
            "-g" | "--glob" => request.globs.push(take_value(args, &mut index, arg)?),
            "--iglob" => request
                .insensitive_globs
                .push(take_value(args, &mut index, arg)?),
            "-t" | "--type" => request.file_types.push(take_value(args, &mut index, arg)?),
            "-T" | "--type-not" => request
                .excluded_file_types
                .push(take_value(args, &mut index, arg)?),
            "--ignore-file" => request
                .ignore_files
                .push(PathBuf::from(take_value(args, &mut index, arg)?)),
            "--max-depth" => request.max_depth = Some(take_usize(args, &mut index, arg)?),
            "--max-filesize" => {
                let value = take_value(args, &mut index, arg)?;
                request.max_file_size_bytes =
                    Some(parse_byte_size(&value).map_err(|_| invalid(arg, &value))?);
            }
            "-A" | "--after-context" => {
                request.rg_options.after_context = take_usize(args, &mut index, arg)?;
            }
            "-B" | "--before-context" => {
                request.rg_options.before_context = take_usize(args, &mut index, arg)?;
            }
            "-C" | "--context" => {
                let value = take_usize(args, &mut index, arg)?;
                request.rg_options.before_context = value;
                request.rg_options.after_context = value;
            }
            "-e" | "--regexp" => request.queries.push(take_value(args, &mut index, arg)?),
            "-f" | "--file" => request
                .rg_options
                .pattern_files
                .push(PathBuf::from(take_value(args, &mut index, arg)?)),
            value if is_output_option(value) => {
                return Err(ManagedRgArgumentError::OutputOption(value.to_owned()));
            }
            value if value.starts_with("--") && flag_without_value(value) => {
                request.rg_options.extra_args.push(value.to_owned());
            }
            value if value.starts_with('-') && value.len() > 2 => {
                parse_short_group(value, args, &mut index, &mut request)?;
            }
            value if value.starts_with('-') => {
                return Err(ManagedRgArgumentError::UnsupportedOption(value.to_owned()));
            }
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }
    if request.queries.is_empty() && request.rg_options.pattern_files.is_empty() {
        if positionals.is_empty() {
            return Err(ManagedRgArgumentError::MissingPattern);
        }
        request.query = Some(positionals.remove(0));
    }
    request.rg_paths = positionals.into_iter().map(PathBuf::from).collect();
    Ok(request)
}

fn parse_long_with_value(
    name: &str,
    value: &str,
    request: &mut ContextOptions,
) -> Result<bool, ManagedRgArgumentError> {
    match name {
        "--glob" => request.globs.push(non_empty(name, value)?),
        "--iglob" => request.insensitive_globs.push(non_empty(name, value)?),
        "--type" => request.file_types.push(non_empty(name, value)?),
        "--type-not" => request.excluded_file_types.push(non_empty(name, value)?),
        "--ignore-file" => request.ignore_files.push(non_empty(name, value)?.into()),
        "--max-depth" => request.max_depth = Some(parse_usize(name, value)?),
        "--max-filesize" => {
            request.max_file_size_bytes =
                Some(parse_byte_size(value).map_err(|_| invalid(name, value))?);
        }
        "--context" => {
            let parsed = parse_usize(name, value)?;
            request.rg_options.before_context = parsed;
            request.rg_options.after_context = parsed;
        }
        "--before-context" => request.rg_options.before_context = parse_usize(name, value)?,
        "--after-context" => request.rg_options.after_context = parse_usize(name, value)?,
        "--regexp" => request.queries.push(non_empty(name, value)?),
        "--file" => request
            .rg_options
            .pattern_files
            .push(non_empty(name, value)?.into()),
        name if is_output_option(name) => {
            return Err(ManagedRgArgumentError::OutputOption(name.to_owned()));
        }
        name if flag_with_value(name) => request
            .rg_options
            .extra_args
            .extend([name.to_owned(), value.to_owned()]),
        _ => return Ok(false),
    }
    Ok(true)
}

fn parse_short_group(
    arg: &str,
    args: &[String],
    index: &mut usize,
    request: &mut ContextOptions,
) -> Result<(), ManagedRgArgumentError> {
    let chars = arg.char_indices().skip(1).collect::<Vec<_>>();
    for (position, (offset, option)) in chars.iter().copied().enumerate() {
        let flag = format!("-{option}");
        match option {
            'n' | 'H' => {}
            'F' => {
                request.rg_options.fixed_strings = true;
            }
            'i' => {
                request.rg_options.ignore_case = true;
            }
            'w' => {
                request.rg_options.word_regexp = true;
            }
            'P' | 'S' | 's' | 'a' | 'u' | 'U' | 'v' | 'x' | 'z' => {
                request.rg_options.extra_args.push(flag);
            }
            'L' => request.follow = true,
            'e' | 'g' | 'E' | 't' | 'T' | 'f' | 'm' | 'j' | 'A' | 'B' | 'C' => {
                let inline_start = offset + option.len_utf8();
                let value = if position + 1 < chars.len() {
                    arg[inline_start..].to_owned()
                } else {
                    take_value(args, index, &flag)?
                };
                match option {
                    'e' => request.queries.push(value),
                    'g' => request.globs.push(value),
                    'E' | 'm' | 'j' => request.rg_options.extra_args.extend([flag, value]),
                    't' => request.file_types.push(value),
                    'T' => request.excluded_file_types.push(value),
                    'f' => request.rg_options.pattern_files.push(value.into()),
                    'A' => request.rg_options.after_context = parse_usize(&flag, &value)?,
                    'B' => request.rg_options.before_context = parse_usize(&flag, &value)?,
                    'C' => {
                        let parsed = parse_usize(&flag, &value)?;
                        request.rg_options.before_context = parsed;
                        request.rg_options.after_context = parsed;
                    }
                    _ => unreachable!("covered short option"),
                }
                return Ok(());
            }
            _ if is_output_option(&flag) => return Err(ManagedRgArgumentError::OutputOption(flag)),
            _ => return Err(ManagedRgArgumentError::UnsupportedOption(flag)),
        }
    }
    Ok(())
}

fn flag_with_value(value: &str) -> bool {
    matches!(
        value,
        "--dfa-size-limit"
            | "--encoding"
            | "--engine"
            | "--max-columns"
            | "--max-count"
            | "--regex-size-limit"
            | "--threads"
    )
}

fn flag_without_value(value: &str) -> bool {
    matches!(
        value,
        "--auto-hybrid-regex"
            | "--case-sensitive"
            | "--binary"
            | "--crlf"
            | "--invert-match"
            | "--line-regexp"
            | "--mmap"
            | "--multiline"
            | "--multiline-dotall"
            | "--no-crlf"
            | "--no-fixed-strings"
            | "--no-ignore-dot"
            | "--no-ignore-files"
            | "--no-ignore-global"
            | "--no-ignore-parent"
            | "--no-ignore-vcs"
            | "--no-config"
            | "--no-mmap"
            | "--no-multiline"
            | "--no-search-zip"
            | "--pcre2"
            | "--one-file-system"
            | "--search-zip"
            | "--smart-case"
            | "--stop-on-nonmatch"
            | "--text"
            | "--unicode"
            | "--no-unicode"
            | "--glob-case-insensitive"
    )
}

fn is_output_option(value: &str) -> bool {
    matches!(
        value.split_once('=').map_or(value, |(name, _)| name),
        "--count"
            | "--count-matches"
            | "--files"
            | "--files-with-matches"
            | "--files-without-match"
            | "--column"
            | "--byte-offset"
            | "--no-column"
            | "--colors"
            | "--context-separator"
            | "--field-context-separator"
            | "--field-match-separator"
            | "--json"
            | "--heading"
            | "--no-heading"
            | "--no-filename"
            | "--no-line-number"
            | "--only-matching"
            | "--passthru"
            | "--path-separator"
            | "--quiet"
            | "--pretty"
            | "--replace"
            | "--stats"
            | "--trim"
            | "--vimgrep"
            | "-c"
            | "-b"
            | "-I"
            | "-l"
            | "-N"
            | "-o"
            | "-p"
            | "-q"
            | "-r"
    )
}

fn take_value(
    args: &[String],
    index: &mut usize,
    option: &str,
) -> Result<String, ManagedRgArgumentError> {
    *index += 1;
    args.get(*index)
        .cloned()
        .filter(|value| !value.is_empty())
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
    parse_usize(option, &value)
}

fn parse_usize(option: &str, value: &str) -> Result<usize, ManagedRgArgumentError> {
    value.parse().map_err(|_| invalid(option, value))
}

fn invalid(option: &str, value: &str) -> ManagedRgArgumentError {
    ManagedRgArgumentError::InvalidOptionValue {
        option: option.to_owned(),
        value: value.to_owned(),
    }
}

fn non_empty(option: &str, value: &str) -> Result<String, ManagedRgArgumentError> {
    if value.is_empty() {
        return Err(ManagedRgArgumentError::MissingOptionValue {
            option: option.to_owned(),
        });
    }
    Ok(value.to_owned())
}
