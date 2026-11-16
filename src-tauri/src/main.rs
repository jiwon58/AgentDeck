#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use agent_deck::{diagnostics, storage::Store, terminal::Terminal};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{Emitter, Manager, WindowEvent};

fn main() {
    std::panic::set_hook(Box::new(|_| {
        diagnostics::init(None);
        diagnostics::record("ERROR", "app.panic", None, None, None);
    }));
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(feature = "test-fixture")]
            let fixture_directory = std::env::var_os("AGENT_DECK_TEST_DATA_DIR")
                .map(std::path::PathBuf::from)
                .filter(|path| path.is_absolute());
            #[cfg(not(feature = "test-fixture"))]
            let fixture_directory: Option<std::path::PathBuf> = None;
            let log_directory = fixture_directory
                .as_ref()
                .map(|path| path.join("logs"))
                .or_else(|| app.path().app_log_dir().ok());
            diagnostics::init(log_directory.as_deref());
            diagnostics::record(
                "INFO",
                "app.starting",
                None,
                None,
                Some(env!("CARGO_PKG_VERSION")),
            );
            let path = match fixture_directory {
                Some(path) => path,
                None => diagnostics::stage(
                    app.path().app_data_dir(),
                    "storage.resolve_directory_failed",
                    None,
                )?,
            };
            diagnostics::stage(
                std::fs::create_dir_all(&path),
                "storage.create_directory_failed",
                None,
            )?;
            let store = diagnostics::stage(
                Store::open(&path.join("deck.db")),
                "storage.open_or_migrate_failed",
                None,
            )
            .map_err(std::io::Error::other)?;
            app.manage(commands::Deck {
                store,
                terminal: Arc::new(Terminal::default()),
                directory: path.to_string_lossy().into_owned(),
                allow_close: AtomicBool::new(false),
                configuration: std::sync::Mutex::new(()),
                hermes_download: std::sync::Mutex::new(None),
            });
            diagnostics::info("app.ready", None);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<commands::Deck>();
                if !state.allow_close.load(Ordering::Acquire) {
                    api.prevent_close();
                    let _ = window.emit("request-quit", ());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::set_session_pin,
            commands::set_session_grouping,
            commands::download_agent,
            commands::install_hermes,
            commands::scan,
            commands::delete_root,
            commands::inspect_executable,
            commands::register_runtime,
            commands::delete_runtime,
            commands::save_router,
            commands::delete_router,
            commands::probe_router,
            commands::list_router_models,
            commands::test_router_model,
            commands::save_preferences,
            commands::import_settings,
            commands::save_gjc_preset,
            commands::delete_gjc_preset,
            commands::preview_gjc_preset,
            commands::start_session,
            commands::close_terminal,
            commands::terminal_events,
            commands::write_terminal,
            commands::resize_terminal,
            commands::status,
            commands::delete_session,
            commands::resolve_session,
            commands::report_ui_fault,
            commands::quit
        ])
        .run(tauri::generate_context!());
    if let Err(e) = app {
        diagnostics::init(None);
        diagnostics::record(
            "ERROR",
            "app.start_or_run_failed",
            None,
            None,
            Some(env!("CARGO_PKG_VERSION")),
        );
        eprintln!("Agent Deck failed: {e}");
        std::process::exit(1);
    } else {
        diagnostics::info("app.exited", None);
    }
}
