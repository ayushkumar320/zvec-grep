//! CLI adapter: parse terminal arguments into Core operations and render replies.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use thiserror::Error;
use zg_engine::{ManagedRgArgumentError, Operation, Outcome, Reply, parse_managed_rg_args};

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
    #[command(subcommand)]
    pub action: ServerAction,
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
        operation: Box<Operation>,
    },
    Server(ServerPlan),
}

#[derive(Debug)]
pub enum ServerPlan {
    On(ServerStartArgs),
    Off(ServerHomeArgs),
    Status(ServerHomeArgs),
    Run(ServerStartArgs),
}

#[derive(Debug, Error)]
pub enum CliError {
    #[error("the Rust POC currently implements only `zg query --rg`")]
    UnsupportedSlice,
    #[error(transparent)]
    ManagedRg(#[from] ManagedRgArgumentError),
}

impl Cli {
    /// Converts terminal arguments into one typed Core operation.
    ///
    /// # Errors
    ///
    /// Returns [`CliError`] when the requested vertical slice or managed-rg
    /// argument is not supported by the current POC.
    pub fn into_plan(self, root: PathBuf) -> Result<CliPlan, CliError> {
        match self.command {
            CommandLine::Query(args) if args.rg => Ok(CliPlan::Execute {
                mode: args.mode,
                operation: Box::new(Operation::lexical(
                    root,
                    parse_managed_rg_args(&args.rg_args)?,
                )),
            }),
            CommandLine::Query(_) => Err(CliError::UnsupportedSlice),
            CommandLine::Server(args) => Ok(CliPlan::Server(match args.action {
                ServerAction::On(args) => ServerPlan::On(args),
                ServerAction::Off(args) => ServerPlan::Off(args),
                ServerAction::Status(args) => ServerPlan::Status(args),
                ServerAction::Run(args) => ServerPlan::Run(args),
            })),
        }
    }
}

/// Renders a canonical Core outcome for the terminal.
///
/// # Errors
///
/// Returns the writer's I/O error when output cannot be written.
pub fn write_outcome(mut writer: impl Write, outcome: &Outcome) -> io::Result<()> {
    match outcome {
        Outcome::Completed(reply) => match reply.as_ref() {
            Reply::Query(reply) => {
                if reply.items.is_empty() {
                    writeln!(writer, "No matches.")?;
                } else {
                    for item in &reply.items {
                        writeln!(writer, "{}", item.relative_path.display())?;
                        writeln!(writer, "  {}", item.content.trim_end())?;
                    }
                }
                Ok(())
            }
            Reply::LexicalSearch(reply) => {
                if reply.matches.is_empty() {
                    writeln!(writer, "No matches.")?;
                    return Ok(());
                }

                let mut previous_path: Option<&std::path::Path> = None;
                for item in &reply.matches {
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
                        item.range.start_line,
                        item.content.trim_end()
                    )?;
                }
                Ok(())
            }
            Reply::Index(reply) => writeln!(
                writer,
                "Indexed generation {}: {} files, {} entities",
                reply.generation, reply.files_scanned, reply.entities_created
            ),
            Reply::Inspect(reply) => writeln!(
                writer,
                "{}: {}",
                reply.root.display(),
                if reply.indexed {
                    "indexed"
                } else {
                    "unindexed"
                }
            ),
            Reply::ChangeIndex(reply) => {
                writeln!(writer, "{}: {:?}", reply.index_path.display(), reply.policy)
            }
            Reply::Job(reply) => {
                for job in &reply.jobs {
                    writeln!(writer, "{}: {:?}", job.id, job.state)?;
                }
                Ok(())
            }
        },
        Outcome::Accepted(receipt) => writeln!(writer, "Job accepted: {}", receipt.id),
        Outcome::InputRequired(challenge) => {
            writeln!(writer, "Authorization required: {}", challenge.reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;

    use super::{Cli, CliPlan, McpToolset, ServerPlan};
    use zg_engine::Command;

    #[test]
    fn parses_managed_rg_into_a_typed_operation() {
        let cli = Cli::try_parse_from([
            "zg", "query", "--mode", "server", "--rg", "-n", "-F", "needle", "src",
        ])
        .expect("CLI should parse");
        let plan = cli
            .into_plan(PathBuf::from("/workspace"))
            .expect("plan should be valid");
        let CliPlan::Execute { operation, .. } = plan else {
            panic!("query must create an execute plan");
        };
        let Command::LexicalSearch(request) = operation.command else {
            panic!("query --rg must create a lexical operation");
        };
        assert_eq!(request.patterns, ["needle"]);
        assert_eq!(request.paths, [PathBuf::from("src")]);
        assert!(request.options.fixed_strings);
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
}
