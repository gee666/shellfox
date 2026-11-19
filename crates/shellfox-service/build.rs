mod wsl_files_asset;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        wsl_files_asset::embed();
    }
}
