// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `--out …` = scriptable capture: no tray, no windows, real exit code.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if quickshot_lib::wants_headless(&args) {
        std::process::exit(quickshot_lib::run_headless(&args));
    }
    quickshot_lib::run()
}
