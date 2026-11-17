fn main() {
    #[cfg(feature = "web")]
    tauri_build::build();
}
