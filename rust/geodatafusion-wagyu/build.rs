//! Compiles the C entry point in `src/wagyu.cpp` against the vendored Wagyu headers.

fn main() {
    println!("cargo:rerun-if-changed=src/wagyu.cpp");
    println!("cargo:rerun-if-changed=vendor/include");
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .include("vendor/include")
        .file("src/wagyu.cpp")
        .warnings(false)
        .compile("gdf_wagyu");
}
