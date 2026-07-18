mod about;
mod accounts;
mod commands;
mod config_io;
mod db;
mod error;
mod hooks;
mod models;
mod notifications;
mod overview;
mod providers;
mod schedule;
mod scheduler;
mod settings;

use std::fs::OpenOptions;
use std::io::Write;
use std::panic;
use std::time::SystemTime;

use serde::Serialize;
use tauri::{Manager, WindowEvent};

use commands::AppState;

#[derive(Serialize)]
struct AppOverview {
    product_name: String,
    supported_adapters: [&'static str; 3],
}

#[tauri::command]
fn get_app_overview() -> AppOverview {
    AppOverview {
        product_name: about::product_name(),
        supported_adapters: ["New API", "Sub2API", "Custom HTTP"],
    }
}

/// Write a panic backtrace to a temp-file log so installed users can
/// share diagnostics when the app crashes on startup.
fn install_panic_logger() {
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let msg = if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else {
            "(no message)".to_string()
        };

        let location = info
            .location()
            .map(|loc| format!("{}:{}:{}", loc.file(), loc.line(), loc.column()))
            .unwrap_or_else(|| "unknown".to_string());

        let body = format!(
            "[{}] panic '{}' at {}\n\n",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs().to_string())
                .unwrap_or_else(|_| "unknown".to_string()),
            msg,
            location,
        );

        if let Ok(mut f) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("zzz-manager-panic.log"))
        {
            let _ = f.write_all(body.as_bytes());
        }

        prev_hook(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Install panic file logger before anything else so early crashes
    // are captured even when the user cannot open devtools.
    install_panic_logger();

    tauri::Builder::default()
        // Single-instance guard: the Tauri docs require this plugin to be
        // registered first. When a user launches the app again, this closure
        // runs in the ALREADY-running instance instead of spawning a second
        // process. We ignore the incoming argv/cwd and just resurface the main
        // window: it may be hidden in the tray or minimized, so we show +
        // unminimize + set_focus to guarantee it becomes visible and focused
        // regardless of its current state.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // Autostart (launch-on-login) plugin. On macOS we register as a
        // LaunchAgent, which is the recommended cross-desktop default; Windows
        // and Linux ignore this argument. No extra launch args are passed, so
        // the app starts exactly as it would from the launcher.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&str>>,
        ))
        .setup(|app| {
            let product_name = about::product_name();
            if let Some(window) = app.get_webview_window("main") {
                window.set_title(&product_name)?;
            }

            // Resolve the app-local data directory and initialize SQLite there.
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
            let db_path = data_dir.join("zzz-manager.db");

            let pool = tauri::async_runtime::block_on(db::init_pool(&db_path)).map_err(|e| {
                Box::new(std::io::Error::other(format!(
                    "init database at {}: {}",
                    db_path.display(),
                    e
                ))) as Box<dyn std::error::Error>
            })?;
            app.manage(AppState { pool: pool.clone() });

            // Start the runtime-only balance-check scheduler. It runs while the
            // app process is alive and stops on exit (ADR-0002).
            scheduler::spawn(pool);

            // Create system tray menu
            let show_item =
                tauri::menu::MenuItemBuilder::with_id("show", "显示主窗口").build(app)?;
            let quit_item = tauri::menu::MenuItemBuilder::with_id("quit", "退出").build(app)?;
            let menu = tauri::menu::MenuBuilder::new(app)
                .item(&show_item)
                .separator()
                .item(&quit_item)
                .build()?;

            // Create system tray icon
            let _tray = tauri::tray::TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip(product_name)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| match event {
                    tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    }
                    | tauri::tray::TrayIconEvent::DoubleClick {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } => {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    _ => {}
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Prevent close and hide window instead
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_overview,
            commands::list_accounts,
            commands::create_account,
            commands::update_account,
            commands::delete_account,
            commands::reorder_accounts,
            commands::get_account_credentials,
            commands::check_account,
            commands::recent_checks,
            commands::query_history,
            commands::get_overview,
            commands::get_schedule_overview,
            commands::get_proxy_settings,
            commands::update_proxy_settings,
            commands::get_app_settings,
            commands::update_app_settings,
            commands::set_scheduler_enabled,
            commands::list_hooks,
            commands::create_hook,
            commands::update_hook,
            commands::delete_hook,
            commands::test_hook,
            commands::recent_deliveries,
            commands::export_config,
            commands::export_config_to_file,
            commands::import_config,
            commands::set_account_enabled,
            commands::get_app_info,
            commands::check_for_update,
            commands::get_autostart_enabled,
            commands::set_autostart_enabled,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
