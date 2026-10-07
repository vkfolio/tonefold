//! The Tonefold app: the standalone build (`cargo run -p tonefold-plugin --features standalone`).
fn main() {
    nih_plug::nih_export_standalone::<tonefold_plugin::Tonefold>();
}
