use std::{error::Error, io, process::ExitCode, sync::Arc, time::Duration};

use clap::Parser;
use tokio::runtime::Builder;
use tokio_util::sync::CancellationToken;
use tracing::debug;
use tracing_subscriber::EnvFilter;
use zg_cli::{Cli, ClientMode};
use zg_engine::{Core, CoreConfig, CorePorts, ResourceBudget, RunControl};
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

    runtime.block_on(async move {
        if plan.mode == ClientMode::Server {
            debug!("managed --rg remains local in server mode");
        }
        let adapter =
            Arc::new(RipgrepAdapter::default().with_max_processes(resources.max_lexical_processes));
        let ports = CorePorts::new().with_lexical(adapter);
        let core = Core::open(CoreConfig::new(ports).with_resources(resources)).await?;

        let cancellation = CancellationToken::new();
        let signal_cancellation = cancellation.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                signal_cancellation.cancel();
            }
        });

        let result = core
            .run(plan.operation, RunControl::local(cancellation))
            .await;
        let shutdown = core.shutdown(Duration::from_secs(5)).await;
        let outcome = result?;
        shutdown?;
        zg_cli::write_outcome(io::stdout().lock(), &outcome)?;
        Ok::<(), Box<dyn Error>>(())
    })
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}
