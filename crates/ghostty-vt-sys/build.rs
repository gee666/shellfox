#[path = "build-support/compiler-path.rs"]
mod compiler_path;
#[path = "build-support/experimental-install.rs"]
mod experimental_install;
#[path = "build-support/install.rs"]
mod install;

use std::{env, error::Error, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rustc-check-cfg=cfg(sf_input_v2_public_probe)");
    for name in [
        "GHOSTTY_VT_DIR",
        "GHOSTTY_VT_LIB_DIR",
        "GHOSTTY_VT_LINK_KIND",
        "GHOSTTY_VT_ENGINE_PROFILE",
        "SHELLFOX_VT_INPUT_V2_PROBE",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for file in [
        "build.rs",
        "build-support",
        "adapter.c",
        "adapter.h",
        "input.c",
        "mouse.c",
        "selection.c",
        "colors.c",
        "recovery.c",
        "presentation.c",
        "presentation.h",
        "metadata.c",
        "metadata-text.c",
        "metadata.h",
        "experimental-native.c",
    ] {
        println!("cargo:rerun-if-changed={file}");
    }
    let experimental = env::var_os("CARGO_FEATURE_EXPERIMENTAL_SHELLFOX_NATIVE").is_some();
    if experimental && env::var_os("GHOSTTY_VT_DIR").is_none() {
        return Err("experimental-shellfox-native requires an explicitly prepared Native002 GHOSTTY_VT_DIR and expected GHOSTTY_VT_ENGINE_PROFILE; official ZIP fallback is disabled".into());
    }
    let target = env::var("TARGET")?;
    let os = env::var("CARGO_CFG_TARGET_OS")?;
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("CARGO_MANIFEST_DIR is missing")?);
    let prefix = match env::var_os("GHOSTTY_VT_DIR") {
        Some(value) if !value.is_empty() => PathBuf::from(value).canonicalize()?,
        Some(_) => return Err("GHOSTTY_VT_DIR must not be empty".into()),
        None if matches!(target.as_str(), "x86_64-pc-windows-msvc" | "x86_64-pc-windows-gnu") => {
            install::windows_archive(&root, &out)?
        }
        None => return Err(format!(
            "libghostty-vt 1.0.1 is not prepared for {target}. Run node crates/ghostty-vt-sys/build-support/prepare.mjs build --help, then set GHOSTTY_VT_DIR to the target-specific installation. Cargo never downloads or silently substitutes another target."
        ).into()),
    };
    let lib = match env::var_os("GHOSTTY_VT_LIB_DIR") {
        Some(value) if !value.is_empty() => {
            if env::var_os("GHOSTTY_VT_DIR").is_none() {
                return Err("GHOSTTY_VT_LIB_DIR requires GHOSTTY_VT_DIR".into());
            }
            PathBuf::from(value).canonicalize()?
        }
        Some(_) => return Err("GHOSTTY_VT_LIB_DIR must not be empty".into()),
        None => prefix.join("lib"),
    };
    let kind = env::var("GHOSTTY_VT_LINK_KIND")
        .unwrap_or_else(|_| if os == "windows" { "dylib" } else { "static" }.into());
    if !matches!(kind.as_str(), "static" | "dylib") {
        return Err("GHOSTTY_VT_LINK_KIND must be static or dylib".into());
    }
    if os == "windows" && kind != "dylib" {
        return Err(
            "Windows requires the official GNU DLL and import library, not the static library"
                .into(),
        );
    }
    if matches!(os.as_str(), "android" | "ios") && kind != "static" {
        return Err("Android and iOS require a prepared static libghostty-vt installation".into());
    }
    install::validate(&prefix, &lib, &target, &os, &kind)?;
    if experimental {
        if os != "windows" && kind != "static" {
            return Err(
                "experimental Native002 currently requires static linking outside Windows".into(),
            );
        }
        let profile = env::var("GHOSTTY_VT_ENGINE_PROFILE").map_err(|_| {
            "experimental-shellfox-native requires expected GHOSTTY_VT_ENGINE_PROFILE"
        })?;
        experimental_install::validate(
            &prefix,
            &lib,
            &target,
            &os,
            &profile,
            &root.join("../.."),
            &out,
        )?;
        println!("cargo:rustc-env=SHELLFOX_GHOSTTY_ENGINE_PROFILE={profile}");
    } else if prefix.join("shellfox-native002.recipe").exists() {
        return Err("experimental Native002 installations require the experimental-shellfox-native feature; refusing to stage a fork DLL in an official/default build".into());
    }
    let mut cc = cc::Build::new();
    cc.files(
        [
            "adapter.c",
            "input.c",
            "mouse.c",
            "selection.c",
            "colors.c",
            "recovery.c",
            "presentation.c",
            "metadata.c",
            "metadata-text.c",
        ]
        .map(|file| compiler_path::for_compiler(&root.join(file))),
    )
    .include(compiler_path::for_compiler(&prefix.join("include")))
    .std("c11")
    .warnings(true);
    if kind == "static" {
        cc.define("GHOSTTY_STATIC", None);
    }
    if experimental {
        cc.define("SHELLFOX_EXPERIMENTAL_NATIVE", "1")
            .file(compiler_path::for_compiler(
                &root.join("experimental-native.c"),
            ));
    }
    let input_v2_probe =
        env::var_os("SHELLFOX_VT_INPUT_V2_PROBE").as_deref() == Some(std::ffi::OsStr::new("1"));
    if input_v2_probe {
        let probe = root.join("tests/input_v2_public.c");
        if !probe.is_file() {
            return Err(
                "SHELLFOX_VT_INPUT_V2_PROBE=1 requires Maurice's tests/input_v2_public.c source"
                    .into(),
            );
        }
        println!("cargo:rerun-if-changed={}", probe.display());
        cc.file(compiler_path::for_compiler(&probe));
    }
    cc.try_compile("shellfox_ghostty_vt_adapter")?;
    if input_v2_probe {
        println!("cargo:rustc-cfg=sf_input_v2_public_probe");
    }
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib={kind}=ghostty-vt");
    println!(
        "cargo:include={}",
        compiler_path::for_compiler(&prefix.join("include")).display()
    );
    println!("cargo:libdir={}", lib.display());
    println!("cargo:version=1.0.1");
    if os == "windows" {
        install::stage_windows_runtime(&prefix, &out, &root.join("build-support/notices"))?;
        println!(
            "cargo:runtime_dll={}",
            prefix.join("bin/ghostty-vt.dll").display()
        );
    }
    Ok(())
}
