use std::path::PathBuf;

fn main() {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let out_dir = write_platform_slint(&target_os);

    let mut config = slint_build::CompilerConfiguration::new().with_include_paths(vec![out_dir]);
    if target_os == "macos" {
        config = config.with_style("cupertino".into());
    }
    slint_build::compile_with_config("ui/main.slint", config).expect("failed to compile Slint UI");

    if target_os == "windows" {
        println!("cargo:rerun-if-changed=assets/app.ico");
        embed_windows_icon();
    }
}

/// Constants the Slint UI reads at compile time, so the Windows frame and the
/// macOS title bar are correct when the window is created.
fn write_platform_slint(target_os: &str) -> PathBuf {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let windows = target_os == "windows";
    let macos = target_os == "macos";
    std::fs::write(
        out_dir.join("platform.slint"),
        format!(
            "export global Platform {{\n    out property <bool> windows: {windows};\n    out property <bool> macos: {macos};\n}}\n"
        ),
    )
    .expect("write platform.slint");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    out_dir
}

#[cfg(target_os = "windows")]
fn embed_windows_icon() {
    winresource::WindowsResource::new()
        .set_icon("assets/app.ico")
        .compile()
        .expect("failed to embed Windows icon resource");
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_icon() {}
