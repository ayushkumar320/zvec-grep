//! CLI adapter: parse terminal arguments into typed requests and render replies.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use thiserror::Error;
use zg_engine::api::context::{ContextOptions, ContextResult, result::ContentRange};

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

        match arg.as_str() {
            "-n" | "--line-number" => {}
            "-F" | "--fixed-strings" => request.rg_options.fixed_strings = true,
            "-i" | "--ignore-case" => request.rg_options.ignore_case = true,
            "-w" | "--word-regexp" => request.rg_options.word_regexp = true,
            "--hidden" => request.hidden = true,
            "--no-ignore" => request.no_ignore = true,
            "--follow" => request.follow = true,
            "-g" | "--glob" => request.globs.push(take_value(args, &mut index, arg)?),
            "-t" | "--type" => request.file_types.push(take_value(args, &mut index, arg)?),
            "-T" | "--type-not" => request
                .excluded_file_types
                .push(take_value(args, &mut index, arg)?),
            "--ignore-file" => request
                .ignore_files
                .push(PathBuf::from(take_value(args, &mut index, arg)?)),
            "--max-depth" => {
                request.max_depth = Some(take_usize(args, &mut index, arg)?);
            }
            "--max-filesize" => {
                request.max_file_size_bytes = Some(take_u64(args, &mut index, arg)?);
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

#[derive(Debug, Parser)]
#[command(name = "zg", version, about = "Agent-friendly workspace search")]
pub struct Cli {
    #[command(subcommand)]
    pub command: CommandLine,
}

#[derive(Debug, Subcommand)]
pub enum CommandLine {
    /// Search indexed context or run managed ripgrep.
    Query(QueryArgs),
    /// Manage the resident MCP daemon.
    Server(ServerArgs),
}

#[derive(Debug, Args)]
pub struct ServerArgs {
    /// Start or reuse the resident daemon and proxy MCP over stdin/stdout.
    #[arg(long)]
    pub stdio: bool,

    /// Loopback HTTP listen address used when stdio needs to start the daemon.
    #[arg(long, value_name = "ADDRESS")]
    pub listen: Option<String>,

    /// zvec-grep state home used by the stdio bridge.
    #[arg(long, env = "ZVEC_GREP_HOME")]
    pub home: Option<PathBuf>,

    /// Public MCP tool profile used by the stdio bridge.
    #[arg(long, env = "ZVEC_GREP_MCP_TOOLSET", value_enum)]
    pub mcp_toolset: Option<McpToolset>,

    #[command(subcommand)]
    pub action: Option<ServerAction>,
}

#[derive(Debug, Subcommand)]
pub enum ServerAction {
    /// Start the resident daemon in the background.
    On(ServerStartArgs),
    /// Stop the resident daemon.
    Off(ServerHomeArgs),
    /// Print resident daemon status.
    Status(ServerHomeArgs),
    /// Run the resident daemon in the foreground (internal).
    #[command(hide = true)]
    Run(ServerStartArgs),
}

#[derive(Clone, Debug, Args)]
pub struct ServerStartArgs {
    /// Loopback HTTP listen address.
    #[arg(long, default_value = "127.0.0.1:7999")]
    pub listen: String,

    /// zvec-grep state home. Defaults to `ZVEC_GREP_HOME` or `~/.zvec-grep`.
    #[arg(long, env = "ZVEC_GREP_HOME")]
    pub home: Option<PathBuf>,

    /// Public MCP tool profile.
    #[arg(
        long,
        env = "ZVEC_GREP_MCP_TOOLSET",
        value_enum,
        default_value = "agent"
    )]
    pub mcp_toolset: McpToolset,
}

#[derive(Clone, Debug, Args)]
pub struct ServerHomeArgs {
    /// zvec-grep state home. Defaults to `ZVEC_GREP_HOME` or `~/.zvec-grep`.
    #[arg(long, env = "ZVEC_GREP_HOME")]
    pub home: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum McpToolset {
    #[default]
    Agent,
    Full,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// Execution mode. Managed --rg remains local in every mode.
    #[arg(long, env = "ZVEC_GREP_MODE", default_value = "auto")]
    pub mode: ClientMode,

    /// Run exhaustive managed ripgrep without opening an index.
    #[arg(long)]
    pub rg: bool,

    /// Ripgrep-compatible options, pattern and paths.
    #[arg(
        value_name = "RG_ARG",
        allow_hyphen_values = true,
        trailing_var_arg = true
    )]
    pub rg_args: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ClientMode {
    Auto,
    Direct,
    Server,
}

#[derive(Debug)]
pub enum CliPlan {
    Execute {
        mode: ClientMode,
        request: Box<ContextOptions>,
    },
    Server(ServerPlan),
}

#[derive(Debug)]
pub enum ServerPlan {
    Stdio(ServerStartArgs),
    On(ServerStartArgs),
    Off(ServerHomeArgs),
    Status(ServerHomeArgs),
    Run(ServerStartArgs),
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("the Rust POC currently implements only `zg query --rg`")]
    UnsupportedSlice,
    #[error("use `zg server --stdio` or choose one of: on, off, status")]
    MissingServerAction,
    #[error("`--stdio` cannot be combined with a server action")]
    StdioWithServerAction,
    #[error(transparent)]
    ManagedRg(#[from] ManagedRgArgumentError),
}

impl Cli {
    /// Converts terminal arguments into one typed engine request.
    ///
    /// # Errors
    ///
    /// Returns [`CliError`] when the requested vertical slice or managed-rg
    /// argument is not supported by the current POC.
    pub fn into_plan(self, root: PathBuf) -> Result<CliPlan, CliError> {
        match self.command {
            CommandLine::Query(args) if args.rg => {
                let mut request = parse_managed_rg_args(&args.rg_args)?;
                request.root = Some(root);
                Ok(CliPlan::Execute {
                    mode: args.mode,
                    request: Box::new(request),
                })
            }
            CommandLine::Query(_) => Err(CliError::UnsupportedSlice),
            CommandLine::Server(args) => {
                let plan = if args.stdio {
                    if args.action.is_some() {
                        return Err(CliError::StdioWithServerAction);
                    }
                    ServerPlan::Stdio(ServerStartArgs {
                        listen: args.listen.unwrap_or_else(|| "127.0.0.1:7999".to_owned()),
                        home: args.home,
                        mcp_toolset: args.mcp_toolset.unwrap_or_default(),
                    })
                } else {
                    match args.action.ok_or(CliError::MissingServerAction)? {
                        ServerAction::On(args) => ServerPlan::On(args),
                        ServerAction::Off(args) => ServerPlan::Off(args),
                        ServerAction::Status(args) => ServerPlan::Status(args),
                        ServerAction::Run(args) => ServerPlan::Run(args),
                    }
                };
                Ok(CliPlan::Server(plan))
            }
        }
    }
}

/// Renders an rg-backed context result for the terminal.
///
/// # Errors
///
/// Returns the writer's I/O error when output cannot be written.
pub fn write_context_result(mut writer: impl Write, result: &ContextResult) -> io::Result<()> {
    if result.items.is_empty() {
        writeln!(writer, "No matches.")?;
        return Ok(());
    }

    let mut previous_path: Option<&std::path::Path> = None;
    for item in &result.items {
        if previous_path != Some(item.relative_path.as_path()) {
            if previous_path.is_some() {
                writeln!(writer)?;
            }
            writeln!(writer, "{}", item.relative_path.display())?;
            previous_path = Some(item.relative_path.as_path());
        }
        writeln!(
            writer,
            "  {}: {}",
            start_line(&item.range),
            item.content.trim_end()
        )?;
    }
    Ok(())
}

fn start_line(range: &ContentRange) -> usize {
    match range {
        ContentRange::Text { start_line, .. } => *start_line,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, CliPlan, McpToolset, ServerPlan};

    #[test]
    fn parses_managed_rg_into_a_typed_request() {
        let cli = Cli::try_parse_from([
            "zg", "query", "--mode", "server", "--rg", "-n", "-F", "needle", "src",
        ])
        .expect("CLI should parse");
        let plan = cli
            .into_plan(PathBuf::from("/workspace"))
            .expect("plan should be valid");
        let CliPlan::Execute { request, .. } = plan else {
            panic!("query must create an execute plan");
        };
        assert_eq!(request.root, Some(PathBuf::from("/workspace")));
        assert_eq!(request.query.as_deref(), Some("needle"));
        assert_eq!(request.rg_paths, [PathBuf::from("src")]);
        assert!(request.rg_options.fixed_strings);
    }

    #[test]
    fn parses_server_on_with_agent_toolset() {
        let cli = Cli::try_parse_from([
            "zg",
            "server",
            "on",
            "--listen",
            "127.0.0.1:8123",
            "--mcp-toolset",
            "agent",
        ])
        .expect("server command should parse");
        let plan = cli
            .into_plan(PathBuf::from("/workspace"))
            .expect("plan should be valid");
        let CliPlan::Server(ServerPlan::On(args)) = plan else {
            panic!("server on must create a server plan");
        };
        assert_eq!(args.listen, "127.0.0.1:8123");
        assert_eq!(args.mcp_toolset, McpToolset::Agent);
    }

    #[test]
    fn parses_full_mcp_toolset() {
        let cli = Cli::try_parse_from(["zg", "server", "on", "--mcp-toolset", "full"])
            .expect("full toolset must be accepted");
        let plan = cli
            .into_plan(PathBuf::from("/workspace"))
            .expect("server plan");
        let CliPlan::Server(ServerPlan::On(args)) = plan else {
            panic!("server on must create a server plan");
        };
        assert_eq!(args.mcp_toolset, McpToolset::Full);
    }

    #[test]
    fn parses_stdio_bootstrap_with_full_toolset() {
        let cli = Cli::try_parse_from([
            "zg",
            "server",
            "--stdio",
            "--listen",
            "127.0.0.1:8124",
            "--home",
            "/tmp/zg-stdio-test",
            "--mcp-toolset",
            "full",
        ])
        .expect("stdio bootstrap should parse");
        let plan = cli
            .into_plan(PathBuf::from("/workspace"))
            .expect("stdio plan");
        let CliPlan::Server(ServerPlan::Stdio(args)) = plan else {
            panic!("server --stdio must create a stdio server plan");
        };
        assert_eq!(args.listen, "127.0.0.1:8124");
        assert_eq!(args.home, Some(PathBuf::from("/tmp/zg-stdio-test")));
        assert_eq!(args.mcp_toolset, McpToolset::Full);
    }

    #[test]
    fn rejects_stdio_combined_with_a_lifecycle_action() {
        let cli = Cli::try_parse_from(["zg", "server", "--stdio", "on"])
            .expect("syntax is parsed before plan validation");
        assert!(
            cli.into_plan(PathBuf::from("/workspace")).is_err(),
            "stdio and lifecycle actions must be mutually exclusive"
        );
    }
}
