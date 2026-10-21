//! Keep source-only checkouts compilable without privately generated fixtures.
use std::path::Path;

fn main() {
    // Watch the parent too: restoring a private directory can preserve older
    // file mtimes while changing the parent's directory entry.
    println!("cargo::rerun-if-changed=.");
    println!("cargo::rustc-check-cfg=cfg(gt2_private_data)");
    println!("cargo::rustc-check-cfg=cfg(gt2_private_assets)");
    let assets = Path::new("assets");
    let fixtures = Path::new("tests/data/original");
    println!("cargo::rerun-if-changed={}", assets.display());
    println!("cargo::rerun-if-changed={}", fixtures.display());
    if assets.join("gt2/course/TC_lisence/scene.ron").is_file()
        && assets.join("vehicle/slip.ron").is_file()
        && assets.join("race/camera.ron").is_file()
    {
        println!("cargo::rustc-cfg=gt2_private_assets");
        if fixtures.is_dir() {
            println!("cargo::rustc-cfg=gt2_private_data");
        }
    }
}
