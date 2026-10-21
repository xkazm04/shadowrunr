//! Shadowrunr console shell. The console is a thin client of the `shadowrunr` daemon
//! (`/rpc` and `/events`); all logic lives in the daemon and the web UI talks to it directly,
//! so this shell only resolves which daemon (see `endpoint`), pins the window's CSP to it, and
//! hosts the window and the URL opener used by "Replay on stage".

pub mod endpoint;

use tauri::utils::config::Csp;
use tauri::WebviewWindowBuilder;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let endpoint = match endpoint::resolve(&args, |k| std::env::var(k).ok()) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("shadowrunr console: {e}");
            std::process::exit(1);
        }
    };

    let mut context = tauri::generate_context!();
    context.config_mut().app.security.csp = Some(Csp::Policy(endpoint::csp_for(&endpoint)));
    let script = endpoint::init_script(&endpoint);

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // tauri.conf.json declares the main window with `create: false`; it is built here
            // so the console learns its daemon before any of its code runs.
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("tauri.conf.json declares no `main` window")?;
            WebviewWindowBuilder::from_config(app.handle(), &config)?
                .initialization_script(&script)
                .build()?;
            Ok(())
        })
        .run(context)
        .expect("error while running the Shadowrunr console");
}
