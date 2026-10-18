fn main() {
    // The publik app token is read at compile time (`option_env!("PUBLIK_APP_TOKEN")`
    // in src/publik.rs); make Cargo rebuild when it changes.
    println!("cargo:rerun-if-env-changed=PUBLIK_APP_TOKEN");
    tauri_build::build();
}
