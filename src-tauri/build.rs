fn main() {
    tauri_build::build();

    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let res_path = std::path::Path::new(&out_dir).join("libresource.a");
        if res_path.exists() {
            let path_str = res_path.to_string_lossy().replace('\\', "/");
            println!("cargo:rustc-link-arg={}", path_str);
        }
    }
}
