// Hide the console window for the release app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use eframe::egui;
use notify::{RecursiveMode, Watcher};

use textswitch::{
    config, config_path,
    editor::{app_icon_data, EditorApp},
    ensure_and_read_config, hook, single_instance,
};

fn background_launch() -> bool {
    std::env::args()
        .skip(1)
        .any(|arg| matches!(arg.as_str(), "--background" | "--startup" | "--minimized"))
}

fn main() {
    let background = background_launch();

    let Some(_instance_guard) = single_instance::acquire() else {
        if !background {
            single_instance::notify_existing();
        }
        return;
    };

    // 1. Load initial config (creates the file with an example on first run).
    let initial_text = ensure_and_read_config();
    let initial = config::parse_config(&initial_text).unwrap_or_default();
    let shared = Arc::new(Mutex::new(initial));

    // 2. Start the keyboard hook (the expansion engine) on its own thread.
    {
        let cfg = shared.clone();
        thread::spawn(move || hook::run(cfg));
    }

    // 3. Watch the config file so external edits also hot-reload the engine.
    let (tx, rx) = channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = tx.send(());
        }
    })
    .expect("failed to create file watcher");
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    }
    {
        let cfg = shared.clone();
        thread::spawn(move || {
            while rx.recv().is_ok() {
                thread::sleep(Duration::from_millis(150));
                while rx.try_recv().is_ok() {}
                let text = std::fs::read_to_string(config_path()).unwrap_or_default();
                match config::parse_config(&text) {
                    Ok(new_cfg) => *cfg.lock().unwrap() = new_cfg,
                    Err(e) => eprintln!("config reload error (keeping previous): {e}"),
                }
            }
        });
    }

    // 4. Run the editor window via eframe. It starts hidden (lives in the tray)
    //    and owns the tray icon + show/hide/quit handling.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("TextSwitch")
            .with_inner_size([825.0, 493.0])
            .with_min_inner_size([680.0, 420.0])
            .with_icon(app_icon_data())
            .with_visible(!background),
        ..Default::default()
    };

    let app_shared = shared.clone();
    let result = eframe::run_native(
        "textswitch",
        options,
        Box::new(move |cc| {
            single_instance::listen_for_show_requests(&cc.egui_ctx);
            Ok(Box::new(EditorApp::new(
                cc,
                app_shared,
                config_path(),
                &initial_text,
            )))
        }),
    );

    if let Err(e) = result {
        eprintln!("editor window error: {e}");
    }
}
