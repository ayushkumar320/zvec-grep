use std::{error::Error, io, process::ExitCode, sync::Arc, time::Duration};

use clap::Parser;
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use zg_cli::{Cli, CliPlan, ClientMode, McpToolset, ServerPlan, ServerStartArgs};
use zg_daemon::{DaemonStatus, ListenAddress, McpToolset as DaemonMcpToolset, ServerConfig};
use zg_engine::{Core, CoreConfig, CorePorts, Operation, ResourceBudget, RunControl};
use zg_host_native::NativeWatcherFactory;
use zg_lexical_rg::RipgrepAdapter;

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

    let resources = ResourceBudget::default();
    let worker_threads = resources.max_cpu_tasks;
    let runtime = Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .max_blocking_threads(resources.max_blocking_tasks)
        .enable_all()
        .build()?;

    runtime.block_on(async move { execute_plan(plan, resources).await })
}

async fn execute_plan(plan: CliPlan, resources: ResourceBudget) -> Result<(), Box<dyn Error>> {
    match plan {
        CliPlan::Execute { mode, operation } => {
            execute_operation(mode, *operation, resources).await
        }
        CliPlan::Server(plan) => execute_server_plan(plan, resources).await,
    }
}

async fn execute_operation(
    mode: ClientMode,
    operation: Operation,
    resources: ResourceBudget,
) -> Result<(), Box<dyn Error>> {
    if mode == ClientMode::Server {
        debug!("managed --rg remains local in server mode");
    }
    let core = open_core(resources).await?;

    let cancellation = CancellationToken::new();
    let signal_cancellation = cancellation.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_cancellation.cancel();
        }
    });

    let result = core.run(operation, RunControl::local(cancellation)).await;
    let shutdown = core.shutdown(Duration::from_secs(5)).await;
    let outcome = result?;
    shutdown?;
    zg_cli::write_outcome(io::stdout().lock(), &outcome)?;
    Ok(())
}

async fn execute_server_plan(
    plan: ServerPlan,
    resources: ResourceBudget,
) -> Result<(), Box<dyn Error>> {
    match plan {
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
            let core = open_core(resources).await?;
            let server_result = zg_daemon::run_server(
                config,
                Arc::new(core.clone()),
                Arc::new(NativeWatcherFactory::default()),
            )
            .await;
            let shutdown_result = core.shutdown(Duration::from_secs(5)).await;
            server_result?;
            shutdown_result?;
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
    };
    Ok(config)
}

async fn open_core(resources: ResourceBudget) -> Result<Core, Box<dyn Error>> {
    let adapter =
        Arc::new(RipgrepAdapter::default().with_max_processes(resources.max_lexical_processes));
    let ports = CorePorts::new().with_lexical(adapter);
    Ok(Core::open(CoreConfig::new(ports).with_resources(resources)).await?)
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
