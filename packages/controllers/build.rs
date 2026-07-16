//! Compile Nefarius ViGEmClient (C++) statically into lanplay-controllers on Windows.
//! Same approach as Sunshine: no separate ViGEmClient.dll required at runtime.

fn main() {
    #[cfg(target_os = "windows")]
    {
        use std::env;
        use std::path::PathBuf;

        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        // packages/controllers -> repo root
        let vigem_root = manifest
            .join("..")
            .join("..")
            .join("third-party")
            .join("ViGEmClient");
        let src = vigem_root.join("src").join("ViGEmClient.cpp");
        let include = vigem_root.join("include");

        if !src.is_file() {
            panic!(
                "ViGEmClient sources missing at {}. Vendor third-party/ViGEmClient (see README).",
                src.display()
            );
        }

        println!("cargo:rerun-if-changed={}", src.display());
        println!("cargo:rerun-if-changed={}", include.display());

        cc::Build::new()
            .cpp(true)
            .std("c++17")
            .file(&src)
            .include(&include)
            // Static link (do not define VIGEM_DYNAMIC)
            .define("WIN32_LEAN_AND_MEAN", None)
            .define("NOMINMAX", None)
            .warnings(false)
            .compile("vigem_client");

        println!("cargo:rustc-link-lib=setupapi");
        println!("cargo:rustc-link-lib=advapi32");
    }
}
