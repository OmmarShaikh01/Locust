pub mod acp;
pub mod core;
pub mod daemon;
pub mod gui;
pub mod ipc;
pub mod logging;
pub mod project;
pub mod redact;
pub mod swarm;

use crate::daemon::DaemonServer;
use crate::ipc::socket::{connect_stream, pipe_name_for_project};
use crate::swarm::config::{DEFAULT_CONFIG_PATH, SwarmConfig};
use clap::Parser;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::thread;

pub static ARGS: OnceLock<Args> = OnceLock::new();

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

    /// Run as persistent background daemon without desktop GUI window
    #[arg(long)]
    pub server: bool,
}

pub fn get_args() -> &'static Args {
    ARGS.get().expect("Arguments have not been initialized yet")
}

/// Load the roster and print every problem. Returns the process exit code.
fn run_check(path: &str) -> ExitCode {
    let path = if path.is_empty() {
        DEFAULT_CONFIG_PATH
    } else {
        path
    };
    let config = match SwarmConfig::load(Path::new(path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("locust: {path}: {e}");
            return ExitCode::from(2);
        }
    };
    let problems = config.validate();
    if problems.is_empty() {
        println!(
            "{path}: ok ({} tabs, {} agents)",
            config.tabs.len(),
            config.agents.len()
        );
        ExitCode::SUCCESS
    } else {
        for p in &problems {
            eprintln!("{path}: {p}");
        }
        ExitCode::from(1)
    }
}

fn run() -> io::Result<()> {
    let global_args = get_args();
    let path_str = if global_args.project.is_empty() {
        "."
    } else {
        global_args.project.as_str()
    };
    let path = Path::new(path_str);

    if global_args.server {
        let project = project::Project::open(path).map_err(io::Error::other)?;
        std::env::set_current_dir(&project.root)?;
        logging::init(global_args.debug);
        logging::log(
            logging::Level::Info,
            &format!("locust daemon started in {}", project.root.display()),
        );
        project::remember_project(&project.root);
        let mut server = DaemonServer::new(project)?;
        server.run_loop();
        return Ok(());
    }

    // Desktop GUI client mode:
    let pipe_name = pipe_name_for_project(path);
    if connect_stream(&pipe_name).is_err() {
        let project = project::Project::open(path).map_err(io::Error::other)?;
        std::env::set_current_dir(&project.root)?;
        logging::init(global_args.debug);
        logging::log(
            logging::Level::Info,
            &format!("locust daemon autospawned in {}", project.root.display()),
        );
        project::remember_project(&project.root);
        let mut server = DaemonServer::new(project)?;
        thread::spawn(move || {
            server.run_loop();
        });
        thread::sleep(std::time::Duration::from_millis(100));
    }

    gui::run_desktop(pipe_name);
    Ok(())
}

fn main() -> ExitCode {
    let args = Args::parse();
    let check = args.check;
    let usage = args.usage;
    let config_path = args.swarm_config.clone();
    if ARGS.set(args).is_err() {
        eprintln!("locust: arguments were already initialized");
        return ExitCode::from(2);
    }

    if usage {
        core::telemetry::print_usage_summary();
        return ExitCode::SUCCESS;
    }

    if check {
        if let Some(dir) = Some(get_args().project.as_str()).filter(|d| !d.is_empty())
            && let Err(e) = std::env::set_current_dir(dir)
        {
            eprintln!("locust: cannot open project directory \"{dir}\": {e}");
            return ExitCode::from(2);
        }
        return run_check(&config_path);
    }

    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("locust: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_check_valid_roster() {
        let code = run_check(DEFAULT_CONFIG_PATH);
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::SUCCESS));
    }

    #[test]
    fn test_run_check_bad_roster_returns_one() {
        let temp_dir =
            std::env::temp_dir().join(format!("locust_test_bad_roster_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&temp_dir);
        let bad_file = temp_dir.join("bad.yaml");
        std::fs::write(
            &bad_file,
            "tabs:\n  A: {grid_x: 1, grid_y: 1}\n  B: {grid_x: 1, grid_y: 1}\nagents: []\n",
        )
        .unwrap();

        let code = run_check(bad_file.to_str().unwrap());
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(1)));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_run_check_missing_roster_returns_two() {
        let code = run_check("non_existent_file_12345.yaml");
        assert_eq!(format!("{code:?}"), format!("{:?}", ExitCode::from(2)));
    }
}
