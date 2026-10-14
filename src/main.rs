mod app;
mod command;
mod connections;
mod report;
mod storage;
mod system;

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--report" || a == "--report-json") {
        let snapshot = system::Snapshot::discover();
        if std::env::args().any(|a| a == "--report-json") {
            println!(
                "{}",
                serde_json::to_string_pretty(&snapshot).expect("serializable snapshot")
            );
        } else {
            println!(
                "{}",
                report::full(&snapshot, &storage::load().unwrap_or_default())
            );
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Mac Link")
            .with_inner_size([1120.0, 800.0])
            .with_min_inner_size([900.0, 650.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Mac Link",
        options,
        Box::new(|cc| Ok(Box::new(app::MacLink::new(cc)))),
    )
}
