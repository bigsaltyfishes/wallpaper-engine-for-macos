use std::{
    env,
    path::{Path, PathBuf},
};

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .expect("wallpaper-core lives under crates/")
        .to_path_buf();
    let owe_source = workspace_root.join("external/open-wallpaper-engine");
    let owe_bindings_header = owe_source.join("src/Platform/Apple/SceneWallpaperBindings.h");
    let owe_bindings_source = owe_source.join("src/Platform/Apple/SceneWallpaperBindings.mm");
    let cmake_out_dir =
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("open_wallpaper_engine");
    let cmake_build_dir = cmake_out_dir.join("build");

    println!("cargo:rerun-if-env-changed=CMAKE");
    println!("cargo:rerun-if-env-changed=CMAKE_BUILD_TYPE");
    println!("cargo:rerun-if-env-changed=OWE_NIX_LIBRARY_PATH");
    println!("cargo:rerun-if-changed={}", owe_source.display());
    println!("cargo:rerun-if-changed={}", owe_bindings_header.display());
    println!("cargo:rerun-if-changed={}", owe_bindings_source.display());

    // Generate bindings before CMake so Rust compilation sees the exact header
    // that the static OWE archive is about to build. The wrapper header exposes
    // only primitive C-compatible parameters around OWE renderer methods.
    generate_owe_backend_bindings(&owe_bindings_header);

    let dst = cmake::Config::new(&owe_source)
        .out_dir(&cmake_out_dir)
        .define("BUILD_TESTING", "OFF")
        .define("BUILD_TESTS", "OFF")
        .define("BUILD_WAYWALLEN", "OFF")
        .define("BUILD_QML", "OFF")
        .build_target("wescene-renderer")
        .build();
    debug_assert_eq!(dst, cmake_out_dir);

    emit_static_link_flags(&cmake_build_dir);
}

fn generate_owe_backend_bindings(header: &Path) {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let bindings_path = out_dir.join("owe_backend_bindings.rs");

    let bindings = bindgen::Builder::default()
        .header(header.display().to_string())
        .allowlist_function("owe_.*")
        .allowlist_type("owe_.*")
        .derive_default(true)
        .generate_comments(true)
        .layout_tests(false)
        // OWE can unwind across this boundary. Safe Rust wrappers catch the
        // unwind and convert it to EngineError::Crash.
        .override_abi(bindgen::Abi::CUnwind, "owe_.*")
        .generate()
        .expect("failed to generate open-wallpaper-engine backend bindings");

    bindings
        .write_to_file(&bindings_path)
        .expect("failed to write open-wallpaper-engine backend bindings");
}

fn emit_static_link_flags(build_dir: &Path) {
    for path in static_link_search_paths(build_dir)
        .into_iter()
        .chain(nix_link_search_paths())
    {
        println!("cargo:rustc-link-search=native={}", path.display());
    }

    // Dependents are listed before their archive dependencies. This keeps the
    // final link compatible with linkers that resolve static archives in order.
    // CMake's wpFs target is INTERFACE-only, so no libwpFs.a is emitted.
    for lib in [
        "wescene-renderer",
        "wpAudio",
        "wpLooper",
        "wpVulkanRender",
        "wpScene",
        "wpVulkan",
        "wpRGraph",
        "wpParticle",
        "wpTimer",
        "wpUtils",
        "spirv-reflect-static",
    ] {
        println!("cargo:rustc-link-lib=static={lib}");
    }

    for lib in [
        "c++",
        "vulkan",
        "lz4",
        "freetype",
        "avformat",
        "avcodec",
        "avutil",
        "swresample",
        "qjs",
        "glslang",
        "SPIRV",
        "glslang-default-resource-limits",
    ] {
        println!("cargo:rustc-link-lib=dylib={lib}");
    }

    for framework in [
        "Accelerate",
        "AppKit",
        "CoreGraphics",
        "CoreMedia",
        "CoreVideo",
        "Foundation",
        "IOSurface",
        "Metal",
        "OpenGL",
        "QuartzCore",
        "VideoToolbox",
    ] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}

fn static_link_search_paths(build_dir: &Path) -> Vec<PathBuf> {
    vec![
        build_dir.join("src"),
        build_dir.join("src/Audio"),
        build_dir.join("src/Looper"),
        build_dir.join("src/Scene"),
        build_dir.join("src/Scene/Particle"),
        build_dir.join("src/Scene/RenderGraph"),
        build_dir.join("src/Scene/Timer"),
        build_dir.join("src/Scene/VulkanRender"),
        build_dir.join("src/Utils"),
        build_dir.join("src/Vulkan"),
        build_dir.join("third_party/spirv_reflect"),
    ]
}

fn nix_link_search_paths() -> Vec<PathBuf> {
    env::var_os("OWE_NIX_LIBRARY_PATH")
        .map(|paths| env::split_paths(&paths).collect())
        .unwrap_or_default()
}
