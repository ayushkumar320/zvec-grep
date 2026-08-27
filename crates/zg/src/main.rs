use std::{error::Error, io, process::ExitCode, sync::Arc};

use clap::Parser;
use tokio::runtime::Builder;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use zg_cli::{Cli, CliPlan, ClientMode, McpToolset, ServerPlan, ServerStartArgs};
use zg_daemon::{DaemonStatus, ListenAddress, McpToolset as DaemonMcpToolset, ServerConfig};
use zg_engine::{LexicalSearchRequest, ZvecGrep};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // Clap handles --help/--version before any async runtime or native adapter is built.
    let cli = Cli::parse();
    let plan = cli.into_plan(std::env::current_dir()?)?;
    init_tracing();

    let runtime = Builder::new_multi_thread().enable_all().build()?;

    runtime.block_on(async move { execute_plan(plan).await })
}

async fn execute_plan(plan: CliPlan) -> Result<(), Box<dyn Error>> {
    match plan {
        CliPlan::Execute { mode, request } => execute_request(mode, *request).await,
        CliPlan::Server(plan) => execute_server_plan(plan).await,
    }
}

async fn execute_request(
    mode: ClientMode,
    request: LexicalSearchRequest,
) -> Result<(), Box<dyn Error>> {
    if mode == ClientMode::Server {
        debug!("managed --rg remains local in server mode");
    }
    let engine = ZvecGrep::new();
    let reply = engine.lexical_search(request).await?;
    engine.close();
    zg_cli::write_lexical_reply(io::stdout().lock(), &reply)?;
    Ok(())
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
            let home = zg_daemon::resolve_home(args.home)?;
            let status = zg_daemon::stop_server(&home, zg_daemon::default_stop_timeout()).await?;
            write_server_status(&status);
        }
        ServerPlan::Status(args) => {
            let home = zg_daemon::resolve_home(args.home)?;
            let status = zg_daemon::server_status(&home).await?;
            write_server_status(&status);
        }
        ServerPlan::Run(args) => {
            let config = server_config(args)?;
            zg_daemon::run_server(config, Arc::new(ZvecGrep::new())).await?;
        }
    }
    Ok(())
}

fn server_config(args: ServerStartArgs) -> Result<ServerConfig, Box<dyn Error>> {
    let listen = args.listen.parse::<ListenAddress>()?;
    let home = zg_daemon::resolve_home(args.home)?;
    let mut config = ServerConfig::new(listen, home);
    config.mcp_toolset = match args.mcp_toolset {
        McpToolset::Agent => DaemonMcpToolset::Agent,
        McpToolset::Full => DaemonMcpToolset::Full,
    };
    Ok(config)
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
