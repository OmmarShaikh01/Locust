pub mod controller;
pub mod model;
pub mod telemetry;
pub mod view;

pub use controller::Controller;
pub use model::{AcpServer, Model};

use clap::Parser;
use std::sync::OnceLock;

pub static ARGS: OnceLock<Args> = OnceLock::new();

/// Simple description of what your TUI application does
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Path to the project directory to open
    #[arg(short, long, default_value = "")]
    pub project: String,

    /// Path to the swarm agents.yaml roster to load in the Swarm tab. Defaults to the
    /// built-in full-software-dev-stack roster if not given.
    #[arg(long, default_value = "")]
    pub swarm_config: String,

    /// Display aggregated swarm token usage and invocations telemetry
    #[arg(long)]
    pub usage: bool,

    /// Write debug lines to .locust/locust.log too (default is info and above).
    #[arg(long)]
    pub debug: bool,

    /// Validate the swarm roster and exit. Exit code 0 means the roster is sound.
    #[arg(long)]
    pub check: bool,
}

pub fn get_args() -> &'static Args {
    ARGS.get().expect("Arguments have not been initialized yet")
}
