use std::{env, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let repo_dir = manifest_dir.parent().expect("harness lives under repo root");
    let default_src_dir = repo_dir.join("../libxml2");
    let src_dir = env::var_os("LIBXML2_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_dir.join(path)
            }
        })
        .unwrap_or(default_src_dir);
    let default_lib_dir = src_dir
        .join("build-uppsala-release")
        .canonicalize()
        .unwrap_or_else(|_| src_dir.join("build-uppsala-release"));
    let lib_dir = env::var_os("LIBXML2_LIB_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_dir.join(path)
            }
        })
        .unwrap_or(default_lib_dir);

    println!("cargo:rerun-if-env-changed=LIBXML2_DIR");
    println!("cargo:rerun-if-env-changed=LIBXML2_LIB_DIR");
    println!("cargo:rerun-if-changed={}", lib_dir.display());
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=xml2");
    println!("cargo:rustc-link-lib=m");
}
