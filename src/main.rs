#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod meta;
mod model;
mod platform;
mod theme;

use eframe::egui;

fn window_icon() -> egui::IconData {
    // Original mark: acid chamfered tile with a void "dock" glyph.
    let n = 64usize;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as i32, y as i32);
            let inside = fx >= 4 && fx < 60 && fy >= 4 && fy < 60 && (fx + fy) >= 18 && (fx + fy) <= 108;
            let glyph = (fx >= 16 && fx < 24 && fy >= 16 && fy < 48)
                || (fx >= 40 && fx < 48 && fy >= 16 && fy < 48)
                || (fx >= 24 && fx < 40 && fy >= 29 && fy < 35);
            let i = (y * n + x) * 4;
            if inside {
                let c = if glyph { [0x0A, 0x0B, 0x0D] } else { [0xD4, 0xFF, 0x1F] };
                px[i..i + 3].copy_from_slice(&c);
                px[i + 3] = 255;
            }
        }
    }
    egui::IconData { rgba: px, width: n as u32, height: n as u32 }
}

/// Release builds have no console, so a crash would otherwise vanish silently.
/// Write it to crash.log next to the layout file and tell the user.
/// `--selftest`: open the window, draw a few frames, exit 0. Used by the
/// installer and CI to confirm the app can start on this machine.
/// Exit codes: 0 ok, 2 window/OpenGL could not be created, 3 timed out.
pub static SELFTEST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn install_crash_log() {
    std::panic::set_hook(Box::new(|info| {
        let path = model::data_path().with_file_name("crash.log");
        let when = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let text = format!("HANGAR v{} crash (unix time {when})\n{info}\n\n", env!("CARGO_PKG_VERSION"));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = f.write_all(text.as_bytes());
        }
        if SELFTEST.load(std::sync::atomic::Ordering::Relaxed) {
            return; // never block an unattended run on a dialog
        }
        platform::message_box(
            "HANGAR stopped",
            &format!("HANGAR hit an internal error and has to close.\nYour layout is saved automatically.\n\nDetails were written to:\n{}\n\n{info}", path.display()),
        );
    }));
}

fn main() {
    install_crash_log();
    let args: Vec<std::path::PathBuf> = std::env::args_os()
        .skip(1)
        .map(std::path::PathBuf::from)
        .map(|p| std::path::absolute(&p).unwrap_or(p))
        .filter(|p| p.exists())
        .collect();

    let selftest = std::env::args().any(|a| a == "--selftest");
    if selftest {
        SELFTEST.store(true, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(30));
            std::process::exit(3);
        });
    }

    // One copy only: two copies would overwrite each other's saves.
    if !selftest && !platform::acquire_instance() {
        app::write_inbox(&args);
        platform::focus_existing();
        return;
    }

    let (saved, _) = model::load(&model::data_path());
    let size = saved.window.unwrap_or([1280.0, 800.0]);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(platform::WINDOW_TITLE)
            .with_inner_size(size)
            .with_min_inner_size([640.0, 420.0])
            .with_maximized(saved.maximized)
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    let result = eframe::run_native(
        "HANGAR",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::HangarApp::new(cc.egui_ctx.clone(), args, selftest)))
        }),
    );
    if selftest {
        std::process::exit(if result.is_ok() { 0 } else { 2 });
    }
    if let Err(e) = result {
        platform::message_box(
            "HANGAR could not start",
            &format!("The window could not be created. HANGAR needs OpenGL 2.0 or newer; updating the graphics driver usually fixes this.\n\n{e}"),
        );
    }
}
