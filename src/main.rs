mod app;
mod backend;
mod detect;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([700.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Universal Package Manager",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
