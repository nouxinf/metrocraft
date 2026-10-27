#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod keys;
mod model;
mod platform;
mod theme;
mod tiles;
mod view;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 560.0])
            .with_app_id("metrocraft")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("embedded icon should decode"),
            ),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "Metrocraft",
        options,
        Box::new(|cc| Ok(Box::new(app::Metrocraft::new(cc)))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Info).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no browser window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("missing #the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#the_canvas_id is not a canvas");

        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(app::Metrocraft::new(cc)))),
            )
            .await;

        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(err) => {
                    loading.set_inner_html(
                        "<p>The map editor crashed. See the browser console for details.</p>",
                    );
                    panic!("failed to start Metrocraft: {err:?}");
                }
            }
        }
    });
}
