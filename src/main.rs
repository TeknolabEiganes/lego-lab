//! LEGO Lab: a native visual programming app for the LEGO Education
//! Computer Science & AI kits.

mod app;
mod executor;
mod hardware;
mod program;
mod project;
mod ui;
mod visual;

fn scan() {
    println!("Scanning for LEGO Education devices…");
    let rt = tokio::runtime::Runtime::new().expect("failed to start Tokio");
    match rt.block_on(hardware::lego::scan_report()) {
        Ok(found) if found.is_empty() => {
            println!("No devices found. Switch them on; their light blinks when they are ready.")
        }
        Ok(found) => found.iter().for_each(|line| println!("  {line}")),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn probe() {
    println!("Connecting to LEGO Education devices (motors will not move)…");
    let rt = tokio::runtime::Runtime::new().expect("failed to start Tokio");
    if let Err(e) = rt.block_on(hardware::lego::probe()) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn main() -> eframe::Result {
    match std::env::args().nth(1).as_deref() {
        Some("--scan") => {
            scan();
            return Ok(());
        }
        Some("--probe") => {
            probe();
            return Ok(());
        }
        _ => {}
    }
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    // Real hardware by default; `--simulator` for the built-in simulator.
    let bluetooth = !args.iter().any(|a| a == "--simulator");
    let open_path = args.iter().find(|a| !a.to_string_lossy().starts_with("--")).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("LEGO Lab")
            .with_app_id("lego-lab")
            .with_inner_size([1360.0, 840.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "LEGO Lab",
        options,
        Box::new(move |cc| Ok(Box::new(app::LegoLabApp::new(cc, open_path, bluetooth)))),
    )
}
