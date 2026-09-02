use std::{error::Error, io, path::Path, process::ExitCode, sync::Arc};

#[cfg(target_os = "macos")]
use std::{ffi::OsString, os::unix::process::CommandExt, process::Command};

use tokio::runtime::Builder;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use zg_cli::{
    Cli, CliPlan, ClientMode, IndexOperation, InstallOutcome, McpInstallTransport, McpToolset,
    ServerPlan, ServerStartArgs,
};
use zg_daemon::{DaemonStatus, ListenAddress, McpToolset as DaemonMcpToolset, ServerConfig};
use zg_daemon_protocol::{DaemonCommand, DaemonReply};
use zg_engine::{EngineError, ZvecGrep, api::context::ContextOptions};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if let Some(error) = error.downcast_ref::<EngineError>() {
                eprintln!("{}", error.report());
            } else {
                eprintln!("Error: {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // Clap handles --help/--version before any async runtime or native adapter is built.
    let cli = Cli::parse();
    let plan = cli.into_plan(std::env::current_dir()?)?;
    match &plan {
        CliPlan::Help(topic) => {
            zg_cli::print_help(topic.as_deref())?;
            return Ok(());
        }
        CliPlan::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    install_darwin_metal_residency_mitigation()?;
    init_tracing();

    let runtime = Builder::new_multi_thread().enable_all().build()?;

    runtime.block_on(async move { execute_plan(plan).await })
}

#[cfg(target_os = "macos")]
fn install_darwin_metal_residency_mitigation() -> io::Result<()> {
    if std::env::var_os("GGML_METAL_NO_RESIDENCY").is_some()
        || std::env::var_os("ZVEC_GREP_METAL_KEEP_RESIDENCY").as_deref()
            == Some(std::ffi::OsStr::new("1"))
    {
        return Ok(());
    }

    // Changing the process environment after Tokio starts is not thread-safe. Re-exec
    // before building the runtime so llama.cpp observes the same Metal default as main.
    let executable = std::env::current_exe()?;
    let arguments = std::env::args_os().skip(1).collect::<Vec<OsString>>();
    let error = Command::new(executable)
        .args(arguments)
        .env("GGML_METAL_NO_RESIDENCY", "1")
        .exec();
    Err(error)
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::unnecessary_wraps)]
fn install_darwin_metal_residency_mitigation() -> io::Result<()> {
    Ok(())
}

async fn execute_plan(plan: CliPlan) -> Result<(), Box<dyn Error>> {
    match plan {
        CliPlan::Query {
            mode,
            home,
            request,
            ..
        } => execute_request(mode, home.as_deref(), *request).await,
        CliPlan::Index {
            mode,
            home,
            operation,
            ..
        } => execute_index(mode, home.as_deref(), operation).await,
        CliPlan::Status {
            mode,
            home,
            request,
            check_ready,
        } => execute_status(mode, home.as_deref(), request, check_ready).await,
        CliPlan::Server(plan) => execute_server_plan(plan).await,
        CliPlan::Install(args) => execute_install_plan(&args).await,
        CliPlan::Uninstall(args) => zg_cli::execute_uninstall(&args).map_err(Into::into),
        CliPlan::Help(_) | CliPlan::Version => Ok(()),
    }
}

async fn execute_install_plan(args: &zg_cli::InstallArgs) -> Result<(), Box<dyn Error>> {
    let outcome = zg_cli::execute_install(args)?;
    if outcome.agent_labels.is_empty() {
        return Ok(());
    }

    let status = if std::env::var("ZVEC_GREP_INSTALL_SKIP_SERVER").as_deref() == Ok("1") {
        None
    } else {
        Some(start_installed_server(&outcome).await?)
    };
    if let Some(status) = &status {
        if status.ready {
            println!("  ✓ Server");
            println!(
                "    ready at {}",
                status
                    .server_url
                    .as_deref()
                    .unwrap_or("http://127.0.0.1:7999/mcp")
            );
        } else {
            println!("  ○ Server");
            println!("    not started; run `zg server on`");
        }
    } else {
        println!("  ○ Server");
        println!("    not started; run `zg server on`");
    }
    if outcome.transport == McpInstallTransport::Stdio {
        println!("  ✓ Connection");
        println!("    stdio; reconnects start the server automatically");
    }
    println!("\nzvec-grep is ready\n");
    println!("  Agents       {}", outcome.agent_labels.join(", "));
    println!("  Remote data  Authorization requested on first remote use");
    println!("\nRestart the selected agents or start a new session to load the integration.");
    Ok(())
}

async fn start_installed_server(outcome: &InstallOutcome) -> Result<DaemonStatus, Box<dyn Error>> {
    let listen = zg_cli::resolve_server_listen()?.parse::<ListenAddress>()?;
    let home = zg_daemon::resolve_home(None)?;
    let mut config = ServerConfig::new(listen, home);
    config.mcp_toolset = match outcome.mcp_toolset {
        Some(McpToolset::Agent) => DaemonMcpToolset::Agent,
        Some(McpToolset::Full) => DaemonMcpToolset::Full,
        None => {
            if let Some(environment) = std::env::var_os("ZVEC_GREP_MCP_TOOLSET") {
                match environment.to_string_lossy().as_ref() {
                    "agent" => DaemonMcpToolset::Agent,
                    "full" => DaemonMcpToolset::Full,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "ZVEC_GREP_MCP_TOOLSET must be agent or full",
                        )
                        .into());
                    }
                }
            } else {
                let current = zg_daemon::server_status(&config.home).await?;
                if current.running && current.mcp_toolset.as_deref() == Some("full") {
                    DaemonMcpToolset::Full
                } else {
                    DaemonMcpToolset::Agent
                }
            }
        }
    };
    let executable = std::env::current_exe()?;
    Ok(zg_daemon::start_server(&executable, &config).await?)
}

async fn execute_request(
    mode: ClientMode,
    home: Option<&Path>,
    request: ContextOptions,
) -> Result<(), Box<dyn Error>> {
    if request.rg {
        if mode == ClientMode::Server {
            debug!("managed --rg remains local in server mode");
        }
        return execute_direct_context(request).await;
    }
    if use_server(mode, home).await? {
        let home = zg_daemon::resolve_home(home.map(Path::to_owned))?;
        let reply = zg_daemon::execute_command(&home, DaemonCommand::Context(request)).await?;
        let DaemonReply::Context(result) = reply else {
            return Err(protocol_mismatch("context"));
        };
        zg_cli::write_context_result(io::stdout().lock(), &result)?;
        return Ok(());
    }
    execute_direct_context(request).await
}

async fn execute_direct_context(request: ContextOptions) -> Result<(), Box<dyn Error>> {
    let engine = ZvecGrep::new();
    let result = engine.context(request).await?;
    engine.close();
    zg_cli::write_context_result(io::stdout().lock(), &result)?;
    Ok(())
}

async fn execute_index(
    mode: ClientMode,
    home: Option<&Path>,
    operation: IndexOperation,
) -> Result<(), Box<dyn Error>> {
    let root = match &operation {
        IndexOperation::Build(request) => request.root.clone(),
        IndexOperation::Drop(request) => request.root.clone(),
    }
    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "index root is required"))?;
    let server = use_server(mode, home).await?;
    match operation {
        IndexOperation::Build(request) => {
            let result = if server {
                let home = zg_daemon::resolve_home(home.map(Path::to_owned))?;
                let reply =
                    zg_daemon::execute_command(&home, DaemonCommand::Index(*request)).await?;
                let DaemonReply::Index(result) = reply else {
                    return Err(protocol_mismatch("index"));
                };
                *result
            } else {
                let engine = ZvecGrep::new();
                let result = engine.index(*request).await?;
                engine.close();
                result
            };
            zg_cli::write_index_result(io::stdout().lock(), &root, &result)?;
        }
        IndexOperation::Drop(request) => {
            let removed = if server {
                let home = zg_daemon::resolve_home(home.map(Path::to_owned))?;
                let reply =
                    zg_daemon::execute_command(&home, DaemonCommand::DropIndex(request)).await?;
                let DaemonReply::DropIndex(removed) = reply else {
                    return Err(protocol_mismatch("drop_index"));
                };
                removed
            } else {
                let engine = ZvecGrep::new();
                let removed = engine.drop_index(request).await?;
                engine.close();
                removed
            };
            println!(
                "Workspace index: {}",
                if removed { "dropped" } else { "missing" }
            );
            println!("Root: {}", root.display());
        }
    }
    Ok(())
}

async fn execute_status(
    mode: ClientMode,
    home: Option<&Path>,
    request: zg_engine::api::info::InfoOptions,
    check_ready: bool,
) -> Result<(), Box<dyn Error>> {
    let result = if use_server(mode, home).await? {
        let home = zg_daemon::resolve_home(home.map(Path::to_owned))?;
        let reply = zg_daemon::execute_command(&home, DaemonCommand::Info(request)).await?;
        let DaemonReply::Info(result) = reply else {
            return Err(protocol_mismatch("info"));
        };
        *result
    } else {
        let engine = ZvecGrep::new();
        let result = engine.info(request).await?;
        engine.close();
        result
    };
    zg_cli::write_info_result(io::stdout().lock(), &result)?;
    let ready = result.indexed
        && result
            .status
            .as_ref()
            .is_none_or(|status| status.files_pending == 0 && status.files_failed == 0);
    if check_ready && !ready {
        return Err(io::Error::other("workspace index is not ready").into());
    }
    Ok(())
}

async fn use_server(mode: ClientMode, home: Option<&Path>) -> Result<bool, Box<dyn Error>> {
    match mode {
        ClientMode::Direct => Ok(false),
        ClientMode::Server => Ok(true),
        ClientMode::Auto => {
            let home = zg_daemon::resolve_home(home.map(Path::to_owned))?;
            Ok(zg_daemon::server_status(&home).await?.ready)
        }
    }
}

fn protocol_mismatch(expected: &str) -> Box<dyn Error> {
    io::Error::other(format!("daemon returned a reply other than {expected}")).into()
}

async fn execute_server_plan(plan: ServerPlan) -> Result<(), Box<dyn Error>> {
    match plan {
        ServerPlan::Stdio(args) => {
            let config = server_config(args)?;
            let executable = std::env::current_exe()?;
            zg_daemon::run_stdio_bridge(&executable, &config).await?;
        }
        ServerPlan::On(args) => {
            let config = server_config(args)?;
            let executable = std::env::current_exe()?;
            let status = zg_daemon::start_server(&executable, &config).await?;
            write_server_status(&status);
        }
        ServerPlan::Off(args) => {
            reject_token_file(args.token_file.as_deref())?;
            let home = zg_daemon::resolve_home(args.home)?;
            let status = zg_daemon::stop_server(&home, zg_daemon::default_stop_timeout()).await?;
            write_server_status(&status);
        }
        ServerPlan::Status(args) => {
            let home = zg_daemon::resolve_home(args.home)?;
            let status = zg_daemon::server_status(&home).await?;
            write_server_status(&status);
            if args.check_ready && !status.ready {
                return Err(io::Error::other("server is not ready").into());
            }
        }
        ServerPlan::Run(args) => {
            let config = server_config(args)?;
            zg_daemon::run_server(config, Arc::new(ZvecGrep::new())).await?;
        }
    }
    Ok(())
}

fn server_config(args: ServerStartArgs) -> Result<ServerConfig, Box<dyn Error>> {
    reject_token_file(args.token_file.as_deref())?;
    let listen = args.listen.parse::<ListenAddress>()?;
    let home = zg_daemon::resolve_home(args.home)?;
    let mut config = ServerConfig::new(listen, home);
    config.mcp_toolset = match args.mcp_toolset {
        McpToolset::Agent => DaemonMcpToolset::Agent,
        McpToolset::Full => DaemonMcpToolset::Full,
    };
    Ok(config)
}

fn reject_token_file(token_file: Option<&Path>) -> Result<(), Box<dyn Error>> {
    if token_file.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "--token-file is not yet supported by the Rust daemon",
        )
        .into());
    }
    Ok(())
}

fn write_server_status(status: &DaemonStatus) {
    let label = if status.ready {
        "ready"
    } else if status.running {
        "starting"
    } else {
        "stopped"
    };
    println!("Server: {label}");
    if let Some(pid) = status.pid {
        println!("PID: {pid}");
    }
    if let Some(url) = &status.server_url {
        println!("URL: {url}");
    }
    if let Some(toolset) = &status.mcp_toolset {
        println!("MCP toolset: {toolset}");
    }
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
