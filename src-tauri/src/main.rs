// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Detect if launched as Chrome Native Messaging host.
    // Chrome passes the extension origin as the first argument, e.g. "chrome-extension://<id>/"
    if args.len() > 1 && args[1].starts_with("chrome-extension://") {
        revivel_companion_lib::run_as_native_messaging_host();
        return;
    }

    revivel_companion_lib::run()
}
