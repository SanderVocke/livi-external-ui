use std::env;
use std::path::PathBuf;

fn main() {
    let mandir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let header = format!("{mandir}/third_party/lv2_external_ui.h");
    println!("cargo:rerun-if-changed={header}");
    let bindings = bindgen::Builder::default()
        .header(header)
        .allowlist_type("LV2_External_UI_Host")
        .allowlist_type("LV2_External_UI_Widget")
        .allowlist_item("LV2_EXTERNAL_UI__Host")
        .clang_arg("-I{mandir}")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Unable to generate bindings");

    let out_path = PathBuf::from(mandir).join("src/bindings.rs");
    bindings
        .write_to_file(&out_path)
        .expect(format!("Couldn't write bindings to {out_path:?}!").as_str());
}
