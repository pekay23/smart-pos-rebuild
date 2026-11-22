fn main() {
    println!("cargo:rerun-if-env-changed=ADVANCED_POS_UPDATE_FEED_URL");
    tauri_build::build()
}
