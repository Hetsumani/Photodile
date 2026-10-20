use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn main() {
    // Compile Slint UI definitions
    slint_build::compile("ui/app.slint").expect("Failed to compile Slint UI definition");

    // Re-run if icon files change
    println!("cargo:rerun-if-changed=images/iconV04.ico");
    println!("cargo:rerun-if-changed=images/iconV04.png");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        embed_windows_icon();
    }
}

fn embed_windows_icon() {
    let out_dir = match env::var("OUT_DIR") {
        Ok(dir) => dir,
        Err(_) => return,
    };
    let out_path = Path::new(&out_dir);
    let ico_path = Path::new("images/iconV04.ico");

    if !ico_path.exists() {
        return;
    }

    // Absolute canonicalized or clean forward-slash path for .rc
    let ico_abs = match fs::canonicalize(ico_path) {
        Ok(p) => p.to_string_lossy().replace(r"\\?\", "").replace('\\', "/"),
        Err(_) => ico_path.to_string_lossy().replace('\\', "/"),
    };

    // Generate .rc resource script referencing iconV04.ico directly
    let rc_path = out_path.join("app.rc");
    let rc_content = format!("1 ICON \"{}\"\n", ico_abs);
    if fs::write(&rc_path, rc_content).is_err() {
        return;
    }

    // Compile resource using windres (available in MinGW GCC toolchain)
    let res_path = out_path.join("app_icon.res");
    let status = Command::new("windres")
        .args([
            rc_path.to_str().unwrap(),
            "-O",
            "coff",
            "-o",
            res_path.to_str().unwrap(),
        ])
        .status();

    if let Ok(status) = status {
        if status.success() {
            println!("cargo:rustc-link-arg={}", res_path.display());
        }
    }
}
