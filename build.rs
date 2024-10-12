#[toml_cfg::toml_config]
pub struct Config {
    #[default("localhost")]
    host: &'static str,
    #[default("")]
    product: &'static str,
    #[default("")]
    client_id: &'static str,
    #[default("")]
    product_id: &'static str,
    #[default("")]
    key: &'static str,
    #[default("")]
    wifi_ssid: &'static str,
    #[default("")]
    wifi_psk: &'static str,
}
fn main() {
    if !std::path::Path::new("cfg.toml").exists() {
        panic!("You need to create a `cfg.toml` file with your Wi-Fi credentials! Use `cfg.toml.example` as a template.");
    }
    embuild::espidf::sysenv::output();
}
