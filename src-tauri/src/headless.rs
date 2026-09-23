//! Scriptable capture without the tray app or any window: runs before Tauri starts,
//! writes the file, prints its path and exits with a meaningful code (see `cli::USAGE`).

use image::RgbaImage;

use crate::capture::{capture_all_monitors, compose_region};
use crate::cli::{self, Format, MonitorSel, Options, Target};
use crate::geom::Rect;
use crate::image_util::{encode_jpeg, encode_png_small};
use crate::{output, settings};

const OK: i32 = 0;
const BAD_ARGS: i32 = 2;
const NOT_FOUND: i32 = 3;
const FAILED: i32 = 4;

struct Shot {
    image: RgbaImage,
    app: String,
    title: String,
}

/// GUI-subsystem builds have no console; borrow the caller's so errors and the saved path show
/// up in PowerShell / cmd.
fn attach_console() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: plain Win32 call; failing (e.g. started from Explorer) is harmless.
        let _ = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }
}

fn capture(target: &Target) -> Result<Shot, (i32, String)> {
    let failed = |e: crate::error::AppError| (FAILED, e.to_string());
    match target {
        Target::Window(text) => capture_window(text),
        Target::Screen(sel) => {
            let frames = capture_all_monitors().map_err(failed)?;
            let frame = match sel {
                MonitorSel::All => {
                    let bounds = Rect::union_all(frames.iter().map(|f| f.monitor.rect()))
                        .ok_or((NOT_FOUND, "no monitors found".to_string()))?;
                    return Ok(Shot {
                        image: compose_region(&frames, bounds).map_err(failed)?,
                        app: String::new(),
                        title: "All monitors".into(),
                    });
                }
                MonitorSel::Primary => frames
                    .iter()
                    .find(|f| f.monitor.is_primary)
                    .or(frames.first()),
                MonitorSel::Index(i) => frames.get(i - 1),
            }
            .ok_or_else(|| {
                (
                    NOT_FOUND,
                    format!("monitor not found ({} available)", frames.len()),
                )
            })?;
            Ok(Shot {
                image: frame.image.clone(),
                app: String::new(),
                title: frame.monitor.name.clone(),
            })
        }
        Target::Rect(rect) => {
            let frames = capture_all_monitors().map_err(failed)?;
            if !frames
                .iter()
                .any(|f| f.monitor.rect().intersect(rect).is_some())
            {
                return Err((NOT_FOUND, "--rect is outside every monitor".into()));
            }
            Ok(Shot {
                image: compose_region(&frames, *rect).map_err(failed)?,
                app: String::new(),
                title: String::new(),
            })
        }
    }
}

/// Topmost visible window whose title or app name contains `text` (case-insensitive).
fn capture_window(text: &str) -> Result<Shot, (i32, String)> {
    let needle = text.to_lowercase();
    let windows = xcap::Window::all().map_err(|e| (FAILED, e.to_string()))?;
    let best = windows
        .into_iter()
        .filter(|w| !w.is_minimized().unwrap_or(false))
        .filter_map(|w| {
            let title = w.title().unwrap_or_default();
            let app = w.app_name().unwrap_or_default();
            let hit =
                title.to_lowercase().contains(&needle) || app.to_lowercase().contains(&needle);
            (hit && app != "QuickShot").then(|| (w.z().unwrap_or(0), w, app, title))
        })
        .max_by_key(|(z, ..)| *z);
    let Some((_, window, app, title)) = best else {
        return Err((NOT_FOUND, format!("no visible window matches {text:?}")));
    };
    let image = window
        .capture_image()
        .map_err(|e| (FAILED, format!("capturing {title:?} failed: {e}")))?;
    Ok(Shot { image, app, title })
}

pub fn run(args: &[String]) -> i32 {
    attach_console();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", cli::USAGE);
        return OK;
    }
    let opts: Options = match cli::parse(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("quickshot: {e}\n\n{}", cli::USAGE);
            return BAD_ARGS;
        }
    };
    if opts.delay > 0 {
        std::thread::sleep(std::time::Duration::from_secs(opts.delay as u64));
    }
    let shot = match capture(&opts.target) {
        Ok(s) => s,
        Err((code, msg)) => {
            eprintln!("quickshot: {msg}");
            return code;
        }
    };
    let (w, h) = shot.image.dimensions();
    let stem = settings::expand_pattern(&opts.name, &shot.app, &shot.title, w, h);
    let target = cli::resolve_output(&opts, &stem, opts.out.is_dir());
    let format = target.format;
    let mut path = target.path;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("quickshot: cannot create {}: {e}", dir.display());
            return FAILED;
        }
        // into a folder: never overwrite; an explicit file name is overwritten like any CLI tool
        if target.into_folder {
            path = output::unique_path(dir, &stem, format.extension());
        }
    }
    let bytes = match format {
        Format::Png => encode_png_small(&shot.image),
        Format::Jpeg => encode_jpeg(&shot.image, 90),
    };
    let written = bytes.and_then(|b| {
        std::fs::write(&path, &b)
            .map(|_| b)
            .map_err(crate::error::AppError::from)
    });
    let bytes = match written {
        Ok(b) => b,
        Err(e) => {
            eprintln!("quickshot: writing {} failed: {e}", path.display());
            return FAILED;
        }
    };
    if opts.copy {
        copy(&shot.image, &bytes, format);
    }
    println!("{}", path.display());
    OK
}

fn copy(img: &RgbaImage, bytes: &[u8], format: Format) {
    #[cfg(windows)]
    {
        // the clipboard wants PNG; re-encode when the file was a JPEG
        let png = match format {
            Format::Png => Ok(bytes.to_vec()),
            Format::Jpeg => crate::image_util::encode_png(img),
        };
        let result = png.and_then(|p| output::write_clipboard_windows(Some((img, &p)), None, None));
        if let Err(e) = result {
            eprintln!("quickshot: copy to clipboard failed: {e}");
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (img, bytes, format);
        eprintln!("quickshot: --copy is only supported on Windows");
    }
}
