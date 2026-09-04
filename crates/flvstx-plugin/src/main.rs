//! Standalone build of the plugin GUI for development (`cargo run -p flvstx-plugin --features standalone`).
fn main() {
    nih_plug::nih_export_standalone::<flvstx_plugin::Flvstx>();
}
