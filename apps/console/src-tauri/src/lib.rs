//! Shadowrunr console shell. The console is a thin client of the `shadowrunr` daemon
//! (`http://127.0.0.1:7717`, `/rpc` and `/events`); all logic lives in the daemon and the
//! web UI talks to it directly, so this shell only hosts the window and the URL opener used
//! by "Replay on stage".

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running the Shadowrunr console");
}
