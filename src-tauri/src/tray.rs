use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::capture;
use crate::error::AppResult;
use crate::settings::Settings;
use crate::state::CaptureMode;
use crate::windows;

pub const TRAY_ID: &str = "main";

pub fn build(app: &AppHandle, settings: &Settings) -> AppResult<()> {
    let hk = &settings.hotkeys;
    let item =
        |id: &str, text: &str, accel: &str| -> tauri::Result<tauri::menu::MenuItem<tauri::Wry>> {
            let mut b = MenuItemBuilder::with_id(id, text);
            if !accel.trim().is_empty() {
                b = b.accelerator(accel);
            }
            b.build(app)
        };
    // the delayed-capture hotkey uses the configured delay; show it on that menu item
    let delay_accel = |secs: u32| {
        if settings.capture_delay_secs == secs {
            hk.delayed_region.as_str()
        } else {
            ""
        }
    };
    let menu = MenuBuilder::new(app)
        .item(&item("capture_region", "Capture region", &hk.region)?)
        .item(&item("capture_window", "Capture window", &hk.window)?)
        .item(&item(
            "capture_fullscreen",
            "Capture full screen",
            &hk.fullscreen,
        )?)
        .item(&item(
            "capture_repeat",
            "Repeat last region",
            &hk.repeat_last,
        )?)
        .item(&item(
            "capture_delay_3",
            "Capture region in 3 s",
            delay_accel(3),
        )?)
        .item(&item(
            "capture_delay_5",
            "Capture region in 5 s",
            delay_accel(5),
        )?)
        .item(&item(
            "capture_delay_10",
            "Capture region in 10 s",
            delay_accel(10),
        )?)
        .separator()
        .item(&item("capture_ocr", "Copy text (OCR)", &hk.ocr)?)
        .item(&item("capture_pin", "Pin region to screen", &hk.pin)?)
        .item(&item("capture_color", "Pick color", &hk.color)?)
        .item(&item("capture_qr", "Read QR code / barcode", &hk.qr)?)
        .item(&item(
            "capture_scroll",
            "Scrolling capture (long page)",
            &hk.scroll,
        )?)
        .item(&item(
            "capture_record",
            if crate::record::is_recording() {
                "Stop GIF recording"
            } else {
                "Record a GIF…"
            },
            &hk.record_gif,
        )?)
        .separator()
        .item(&item("capture_watch", "Watch a region…", &hk.watch)?)
        .item(&item(
            "steps_toggle",
            &if crate::steps::is_recording() {
                format!("Stop recording steps ({})", crate::steps::state().count)
            } else {
                "Record steps…".to_string()
            },
            &hk.record_steps,
        )?)
        .item(&item(
            "open_watches",
            &match crate::watch::list().len() {
                0 => "Watches…".to_string(),
                n => format!("Watches ({n} running)…"),
            },
            "",
        )?)
        .separator()
        .item(&item("open_history", "History…", &hk.history)?)
        .item(&item("open_guides", "Guides…", "")?)
        .item(&item("open_settings", "Settings…", "")?)
        .separator()
        .item(&item("quit", "Quit QuickShot", "")?)
        .build()?;

    // macOS: a monochrome template glyph that follows the light/dark menu bar.
    // Windows/Linux: the colored app icon, legible on light and dark taskbars.
    #[cfg(target_os = "macos")]
    let icon = tauri::include_image!("icons/tray.png");
    #[cfg(not(target_os = "macos"))]
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| crate::error::AppError::Other("missing app icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("QuickShot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture_region" => capture::trigger(app, CaptureMode::Region),
            "capture_window" => capture::trigger(app, CaptureMode::Window),
            "capture_fullscreen" => capture::trigger(app, CaptureMode::Fullscreen),
            "capture_repeat" => capture::trigger(app, CaptureMode::RepeatLast),
            "capture_delay_3" => capture::trigger_delayed(app, CaptureMode::Region, 3),
            "capture_delay_5" => capture::trigger_delayed(app, CaptureMode::Region, 5),
            "capture_delay_10" => capture::trigger_delayed(app, CaptureMode::Region, 10),
            "capture_ocr" => capture::trigger(app, CaptureMode::Ocr),
            "capture_pin" => capture::trigger(app, CaptureMode::Pin),
            "capture_color" => capture::trigger(app, CaptureMode::Color),
            "capture_qr" => capture::trigger(app, CaptureMode::Qr),
            "capture_watch" => capture::trigger(app, CaptureMode::Watch),
            "capture_scroll" => capture::trigger(app, CaptureMode::Scroll),
            "capture_record" => {
                if crate::record::is_recording() {
                    crate::record::stop();
                } else {
                    capture::trigger(app, CaptureMode::Record);
                }
            }
            "steps_toggle" => crate::steps::toggle(app),
            "open_watches" => windows::open_watches(app),
            "open_history" => windows::open_history(app),
            "open_guides" => windows::open_guide(app),
            "open_settings" => windows::show_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                capture::trigger(tray.app_handle(), CaptureMode::Region);
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild the tray menu so accelerator hints follow the settings.
pub fn refresh(app: &AppHandle, settings: &Settings) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(false);
        drop(tray);
        let _ = app.remove_tray_by_id(TRAY_ID);
    }
    if let Err(e) = build(app, settings) {
        log::error!("tray rebuild failed: {e}");
    }
}
