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
        .separator()
        .item(&item("capture_ocr", "Copy text (OCR)", &hk.ocr)?)
        .item(&item("capture_pin", "Pin region to screen", &hk.pin)?)
        .item(&item("capture_color", "Pick color", &hk.color)?)
        .separator()
        .item(&item("open_guides", "Guides…", "")?)
        .item(&item("open_settings", "Settings…", "")?)
        .separator()
        .item(&item("quit", "Quit QuickShot", "")?)
        .build()?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| crate::error::AppError::Other("missing app icon".into()))?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip("QuickShot")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture_region" => capture::trigger(app, CaptureMode::Region),
            "capture_window" => capture::trigger(app, CaptureMode::Window),
            "capture_fullscreen" => capture::trigger(app, CaptureMode::Fullscreen),
            "capture_repeat" => capture::trigger(app, CaptureMode::RepeatLast),
            "capture_ocr" => capture::trigger(app, CaptureMode::Ocr),
            "capture_pin" => capture::trigger(app, CaptureMode::Pin),
            "capture_color" => capture::trigger(app, CaptureMode::Color),
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
