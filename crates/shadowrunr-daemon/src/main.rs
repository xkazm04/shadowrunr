//! `shadowrunr`: the Shadowrunr OS command line and daemon. Commands: docs/guide/cli.md.

fn main() -> std::process::ExitCode {
    shadowrunr_daemon::cli::main()
}
