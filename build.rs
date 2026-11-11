use std::{env, path::PathBuf};

fn main() {
    let sdk = PathBuf::from("vendor/wooting-rgb-sdk");
    let hid = sdk.join("hidapi");
    assert!(
        sdk.join("src/wooting-rgb-sdk.c").exists() && hid.join("hidapi/hidapi.h").exists(),
        "Missing SDK sources. Run git submodule update --init --recursive"
    );
    println!("cargo:rerun-if-changed=vendor/wooting-rgb-sdk/src");
    println!("cargo:rerun-if-changed=vendor/wooting-rgb-sdk/hidapi");

    let mut native = cc::Build::new();
    native
        .include(sdk.join("src"))
        .include(hid.join("hidapi"))
        .file(sdk.join("src/wooting-rgb-sdk.c"))
        .file(sdk.join("src/wooting-usb.c"))
        .define("WOOTINGRGBSDK_EXPORTS", None)
        .define("HID_API_NO_EXPORT_DEFINE", None);
    match env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "macos" => {
            // Apple's IOKit headers require Clang's blocks extension.
            native.compiler("clang").file(hid.join("mac/hid.c"));
            for framework in ["IOKit", "CoreFoundation", "AppKit"] {
                println!("cargo:rustc-link-lib=framework={framework}");
            }
        }
        "windows" => {
            native
                .file(hid.join("windows/hid.c"))
                .file(hid.join("windows/hidapi_descriptor_reconstruct.c"));
        }
        "linux" => {
            native.file(hid.join("linux/hid.c"));
            let udev = pkg_config::probe_library("libudev")
                .expect("Install libudev development headers and pkg-config (Ubuntu: libudev-dev pkg-config)");
            for path in udev.include_paths {
                native.include(path);
            }
        }
        os => {
            panic!("Unsupported target OS: {os}. Supported targets are Windows, macOS and Linux.")
        }
    }
    native.compile("wooting-rgb-sdk");

    bindgen::Builder::default()
        .header(sdk.join("src/wooting-rgb-sdk.h").to_string_lossy())
        .clang_arg("-DWOOTINGRGBSDK_EXPORTS")
        .allowlist_function("wooting_.*")
        .allowlist_type("WOOTING_.*|RGB_PARTS|void_cb")
        .allowlist_var("WOOTING_.*|RGB_RAW_BUFFER_SIZE")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("Cannot generate SDK bindings. Install libclang or set LIBCLANG_PATH.")
        .write_to_file(PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bindings.rs"))
        .expect("Cannot write SDK bindings");
}
