fn main() {
    // The publik app token is read at compile time (`option_env!("PUBLIK_APP_TOKEN")`
    // in src/publik.rs); make Cargo rebuild when it changes.
    println!("cargo:rerun-if-env-changed=PUBLIK_APP_TOKEN");
    // Test builds point the updater at a test feed (src/updates.rs, option_env!).
    println!("cargo:rerun-if-env-changed=PUBLIK_UPDATE_ENDPOINT");
    tauri_build::build();
}
