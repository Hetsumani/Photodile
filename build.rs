use std::env;

fn main() {
    // Compile Slint UI definitions
    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI definition");

    // Re-run if icon files change
    println!("cargo:rerun-if-changed=assets/photodile.ico");
    println!("cargo:rerun-if-changed=images/iconV04.ico");
    println!("cargo:rerun-if-changed=images/iconV04.png");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_windows_metadata();
    }
}

#[cfg(windows)]
fn embed_windows_metadata() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/photodile.ico");
    res.set("ProductName", "Photodile");
    res.set("FileDescription", "Photodile");
    res.set("CompanyName", "Hetsumani Interactive");
    if let Err(e) = res.compile() {
        eprintln!("Warning: Failed to compile Windows resource: {}", e);
    }
}

#[cfg(not(windows))]
fn embed_windows_metadata() {}

