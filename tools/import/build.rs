fn main() {
    let fixture = "../../game/tests/data/original/course/scenery_transforms.json";
    println!("cargo::rustc-check-cfg=cfg(gt2_scenery_fixture)");
    println!("cargo::rerun-if-changed={fixture}");
    println!("cargo::rerun-if-changed=../../game/tests/data");
    if std::path::Path::new(fixture).is_file() {
        println!("cargo::rustc-cfg=gt2_scenery_fixture");
    }
}
