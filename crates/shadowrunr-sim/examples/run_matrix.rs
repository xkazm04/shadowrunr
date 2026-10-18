//! Run a tournament matrix against the live Claude Code fleet, before the `shadowrunr` CLI
//! exists. Uses the default login as the single fleet account.
//!
//! cargo run -p shadowrunr-sim --example run_matrix -- sim/matrices/pilot.toml [runs/sim] [max_runs]

use std::path::PathBuf;

use shadowrunr_engine::{build_engine, AccountConfig, EngineConfig};
use shadowrunr_sim::{run_tournament, TournamentOptions};

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let matrix = PathBuf::from(args.next().expect("matrix path"));
    let out = PathBuf::from(args.next().unwrap_or_else(|| "runs/sim".into()));
    let max_runs = args.next().map(|s| s.parse::<usize>().expect("max_runs"));

    let cfg = EngineConfig {
        accounts: vec![AccountConfig {
            label: "main".into(),
            config_dir: None,
        }],
        // Live CLI processes across all concurrent runs (SHADOWRUNR_MAX_LIVE, default 10).
        max_live_processes: Some(
            std::env::var("SHADOWRUNR_MAX_LIVE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
        ),
        ..EngineConfig::default()
    };
    let engine = build_engine(&cfg);
    let opts = TournamentOptions {
        max_runs,
        ..TournamentOptions::default()
    };
    match run_tournament(&matrix, &out, engine, &opts).await {
        Ok(summary) => println!("{summary:#?}"),
        Err(e) => {
            eprintln!("tournament failed: {e}");
            std::process::exit(1);
        }
    }
}
