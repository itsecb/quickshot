mod capture;
mod commands;
mod error;
mod geom;
mod hotkeys;
mod image_util;
mod ocr;
mod output;
mod overlay;
mod protocol;
mod settings;
mod state;
mod tray;
mod windows;

use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use state::{AppState, CaptureMode};

/// `--capture <mode>` / `--settings` / `--guides` / `--hidden`
fn handle_cli(app: &AppHandle, args: &[String]) {
    let mut it = args.iter();
    let mut did_something = false;
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--capture" | "-c" => {
                if let Some(mode) = it.next().and_then(|m| CaptureMode::parse(m)) {
                    capture::trigger(app, mode);
                    did_something = true;
                }
            }
            "--settings" => {
                windows::show_main(app);
                did_something = true;
            }
            "--guides" => {
                windows::open_guide(app);
                did_something = true;
            }
            "--hidden" => did_something = true,
            _ => {}
        }
    }
    if !did_something && args.iter().any(|a| !a.starts_with("--")) {
        // launched again without flags: bring up the settings window
        windows::show_main(app);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args: Vec<String> = argv.into_iter().skip(1).collect();
            if args.is_empty() {
                windows::show_main(app);
            } else {
                handle_cli(app, &args);
            }
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                })
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_drag::init())
        .register_uri_scheme_protocol(protocol::SCHEME, protocol::handle)
        .setup(|app| {
            let handle = app.handle().clone();
            let settings = settings::load(&handle);
            app.manage(AppState::new(settings.clone()));

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::build(&handle, &settings)?;
            let conflicts = hotkeys::register_all(&handle, &settings);
            if !conflicts.is_empty() {
                log::warn!("hotkey conflicts: {conflicts:?}");
            }
            commands::settings::apply_autostart(&handle, settings.autostart);
            output::cleanup_temp(&handle);

            let args: Vec<String> = std::env::args().skip(1).collect();
            let hidden = args.iter().any(|a| a == "--hidden");
            handle_cli(&handle, &args);
            if !hidden && args.is_empty() {
                // First launch by hand: show the settings window so the user sees the app exists.
                windows::show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            if let WindowEvent::Destroyed = event {
                // free capture memory when its editor or pin goes away
                let label = window.label().to_string();
                if label.starts_with("editor-") || label.starts_with("pin-") {
                    if let Some(id) = label.rsplit('-').next().and_then(|s| s.parse::<u64>().ok()) {
                        let still_used = window
                            .app_handle()
                            .webview_windows()
                            .keys()
                            .any(|l| l != &label && l.ends_with(&format!("-{id}")));
                        if !still_used {
                            window.app_handle().state::<AppState>().remove_capture(id);
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::capture::overlay_init,
            commands::capture::overlay_ready,
            commands::capture::finish_capture,
            commands::capture::cancel_capture,
            commands::capture::trigger_capture,
            commands::output::editor_init,
            commands::output::release_capture,
            commands::output::copy_image,
            commands::output::copy_text,
            commands::output::save_image,
            commands::output::export_temp_png,
            commands::output::pin_image,
            commands::output::pin_init,
            commands::output::copy_capture,
            commands::output::save_capture,
            commands::output::capture_pixels,
            commands::output::ocr_capture,
            commands::output::ocr_png,
            commands::settings::get_settings,
            commands::settings::set_settings,
            commands::settings::app_paths,
            commands::settings::open_window,
            commands::settings::hide_main,
            commands::files::fs_write,
            commands::files::fs_write_text,
            commands::files::fs_read_text,
            commands::files::fs_read_bytes,
            commands::files::fs_exists,
            commands::files::fs_mkdir,
            commands::files::fs_remove,
            commands::files::fs_list,
            commands::files::guides_dir,
            commands::guide::guide_push_step,
            commands::guide::guide_pull_steps,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app, event| {
        if let RunEvent::ExitRequested { code, api, .. } = event {
            // Keep running in the tray when the last window closes; exit only on explicit quit.
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
