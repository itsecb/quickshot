//! Argument parsing for headless (scriptable) capture: `quickshot --capture … --out …`.
//! Pure so it can be unit-tested; the capture itself lives in `headless.rs`.

use std::path::{Path, PathBuf};

use crate::geom::Rect;

pub const USAGE: &str = "\
QuickShot scriptable capture (no windows, waits until the file is written):

  quickshot --out <file|folder> [target] [options]

Targets (default: full screen of the primary monitor):
  --capture fullscreen          --monitor primary|all|<n>   (1 = first monitor)
  --window \"<title or app>\"     topmost window whose title or app contains the text
  --rect x,y,width,height       exact area in desktop pixels

Options:
  --format png|jpg     default: from the --out extension, else png
  --name \"<pattern>\"   file name when --out is a folder (tokens: {date} {time} {datetime} {app} {title} {w} {h})
  --delay <seconds>    wait before capturing (0-60)
  --copy               also put the image on the clipboard

Exit codes: 0 saved (path printed), 2 bad arguments, 3 monitor/window not found, 4 capture or write failed.
From PowerShell, wait for it with:  quickshot --out C:\\shots\\ | Out-Null
";

#[derive(Debug, Clone, PartialEq)]
pub enum MonitorSel {
    Primary,
    All,
    /// 1-based, in the order the OS lists monitors.
    Index(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Screen(MonitorSel),
    Window(String),
    Rect(Rect),
}

#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Options {
    pub target: Target,
    pub out: PathBuf,
    pub format: Option<Format>,
    pub name: String,
    pub delay: u32,
    pub copy: bool,
}

/// Headless mode is chosen by `--out` (or asking for help); everything else keeps the tray app.
pub fn wants_headless(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--out" || a == "--help" || a == "-h")
}

fn parse_format(v: &str) -> Result<Format, String> {
    match v.to_ascii_lowercase().as_str() {
        "png" => Ok(Format::Png),
        "jpg" | "jpeg" => Ok(Format::Jpeg),
        _ => Err(format!("unknown format {v:?} (use png or jpg)")),
    }
}

fn parse_rect(v: &str) -> Result<Rect, String> {
    let parts: Vec<&str> = v.split(',').map(str::trim).collect();
    let bad = || format!("--rect needs x,y,width,height, got {v:?}");
    if parts.len() != 4 {
        return Err(bad());
    }
    let x = parts[0].parse::<i32>().map_err(|_| bad())?;
    let y = parts[1].parse::<i32>().map_err(|_| bad())?;
    let w = parts[2].parse::<u32>().map_err(|_| bad())?;
    let h = parts[3].parse::<u32>().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err("--rect width and height must be > 0".into());
    }
    Ok(Rect::new(x, y, w, h))
}

pub fn parse(args: &[String]) -> Result<Options, String> {
    let mut out: Option<PathBuf> = None;
    let mut capture: Option<String> = None;
    let mut monitor: Option<MonitorSel> = None;
    let mut window: Option<String> = None;
    let mut rect: Option<Rect> = None;
    let mut format = None;
    let mut name = "Screenshot {date} {time}".to_string();
    let mut delay = 0;
    let mut copy = false;

    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(value("--out")?)),
            "--capture" | "-c" => capture = Some(value("--capture")?.to_ascii_lowercase()),
            "--monitor" => {
                let v = value("--monitor")?.to_ascii_lowercase();
                monitor = Some(match v.as_str() {
                    "primary" => MonitorSel::Primary,
                    "all" => MonitorSel::All,
                    n => match n.parse::<usize>() {
                        Ok(i) if i >= 1 => MonitorSel::Index(i),
                        _ => {
                            return Err(format!(
                                "--monitor must be primary, all or 1, 2, …; got {v:?}"
                            ))
                        }
                    },
                });
            }
            "--window" => window = Some(value("--window")?),
            "--rect" => rect = Some(parse_rect(&value("--rect")?)?),
            "--format" => format = Some(parse_format(&value("--format")?)?),
            "--name" => name = value("--name")?,
            "--delay" => {
                let v = value("--delay")?;
                delay = v
                    .parse::<u32>()
                    .ok()
                    .filter(|d| *d <= 60)
                    .ok_or_else(|| format!("--delay must be 0-60 seconds, got {v:?}"))?;
            }
            "--copy" => copy = true,
            "--hidden" => {}
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    let out = out.ok_or("--out is required")?;
    let target = match (capture.as_deref(), window, rect) {
        (_, Some(_), Some(_)) => return Err("use either --window or --rect, not both".into()),
        (_, Some(w), None) if w.trim().is_empty() => return Err("--window text is empty".into()),
        (Some("region" | "ocr" | "pin" | "color" | "qr"), _, _) => {
            return Err("interactive modes need the app; use --rect for a fixed area".into())
        }
        (_, Some(w), None) => Target::Window(w),
        (_, None, Some(r)) => Target::Rect(r),
        (Some("window"), None, None) => {
            return Err("--capture window needs --window \"<text>\"".into())
        }
        (None | Some("fullscreen" | "full" | "screen"), None, None) => {
            Target::Screen(monitor.unwrap_or(MonitorSel::Primary))
        }
        (Some(other), None, None) => return Err(format!("unknown --capture mode {other:?}")),
    };
    Ok(Options {
        target,
        out,
        format,
        name,
        delay,
        copy,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    pub path: PathBuf,
    pub format: Format,
    /// `--out` named a folder: the caller should pick a unique name instead of overwriting.
    pub into_folder: bool,
}

/// Final file path and format. `out` is a folder when it exists as one or ends with a slash;
/// then `stem` (already expanded from --name) is used. Format: --format, else the extension.
pub fn resolve_output(opts: &Options, stem: &str, out_is_dir: bool) -> Output {
    let ext_format = |p: &Path| {
        p.extension()
            .and_then(|e| e.to_str())
            .and_then(|e| parse_format(e).ok())
    };
    let raw = opts.out.to_string_lossy();
    let looks_dir = out_is_dir || raw.ends_with('/') || raw.ends_with('\\');
    if looks_dir {
        let format = opts.format.unwrap_or(Format::Png);
        Output {
            path: opts.out.join(format!("{stem}.{}", format.extension())),
            format,
            into_folder: true,
        }
    } else {
        let format = opts
            .format
            .or_else(|| ext_format(&opts.out))
            .unwrap_or(Format::Png);
        Output {
            path: opts.out.clone(),
            format,
            into_folder: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn headless_only_with_out() {
        assert!(wants_headless(&args(&[
            "--capture",
            "fullscreen",
            "--out",
            "x.png"
        ])));
        assert!(!wants_headless(&args(&["--capture", "region"])));
    }

    #[test]
    fn defaults_to_primary_screen() {
        let o = parse(&args(&["--out", "C:\\shots\\"])).unwrap();
        assert_eq!(o.target, Target::Screen(MonitorSel::Primary));
        assert_eq!(o.delay, 0);
        assert!(!o.copy);
    }

    #[test]
    fn parses_every_target() {
        let o = parse(&args(&[
            "--capture",
            "fullscreen",
            "--monitor",
            "all",
            "--out",
            "a.png",
        ]))
        .unwrap();
        assert_eq!(o.target, Target::Screen(MonitorSel::All));
        let o = parse(&args(&["--monitor", "2", "--out", "a.png"])).unwrap();
        assert_eq!(o.target, Target::Screen(MonitorSel::Index(2)));
        let o = parse(&args(&[
            "--window", "Grafana", "--out", "a.png", "--copy", "--delay", "3",
        ]))
        .unwrap();
        assert_eq!(o.target, Target::Window("Grafana".into()));
        assert!(o.copy);
        assert_eq!(o.delay, 3);
        let o = parse(&args(&["--rect", "10, -20,300,200", "--out", "a.png"])).unwrap();
        assert_eq!(o.target, Target::Rect(Rect::new(10, -20, 300, 200)));
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            &["--capture", "fullscreen"][..],
            &["--out"],
            &["--out", "a.png", "--monitor", "0"],
            &["--out", "a.png", "--rect", "1,2,3"],
            &["--out", "a.png", "--rect", "1,2,0,5"],
            &["--out", "a.png", "--delay", "999"],
            &["--out", "a.png", "--format", "gif"],
            &["--out", "a.png", "--capture", "region"],
            &["--out", "a.png", "--capture", "window"],
            &["--out", "a.png", "--window", "x", "--rect", "1,1,1,1"],
            &["--out", "a.png", "--bogus"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?} should fail");
        }
    }

    #[test]
    fn output_path_and_format() {
        let o = parse(&args(&["--out", "C:\\shots\\"])).unwrap();
        let r = resolve_output(&o, "Screenshot 2026-09-22 10-00-00", false);
        assert_eq!(r.format, Format::Png);
        assert!(r.into_folder);
        assert!(r
            .path
            .to_string_lossy()
            .ends_with("Screenshot 2026-09-22 10-00-00.png"));

        let o = parse(&args(&["--out", "dash.JPG"])).unwrap();
        assert_eq!(
            resolve_output(&o, "x", false),
            Output {
                path: PathBuf::from("dash.JPG"),
                format: Format::Jpeg,
                into_folder: false
            }
        );

        let o = parse(&args(&["--out", "shots", "--format", "jpg"])).unwrap();
        let r = resolve_output(&o, "s", true);
        assert_eq!(r.path, PathBuf::from("shots").join("s.jpg"));
        assert!(r.into_folder);
    }
}
