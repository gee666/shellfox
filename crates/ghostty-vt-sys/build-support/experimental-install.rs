use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    error::Error,
    fs,
    io::{BufRead, BufReader, Read},
    path::Path,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

const RECIPE: &str = "shellfox-native002.recipe";
const FILES: &str = "shellfox-native002.files";
const FIXED: &[(&str, &str)] = &[
    ("shellfox-native002", "1"),
    (
        "source",
        "333dfce97641ef9b536bb9e08907ad86abaaada50e8f7417b70d9284adef37aa",
    ),
    (
        "deps",
        "91a4e10960a52bf5bf4b15aa2a2cddaff8a89d6cd903d3055bda4fd17de3356d",
    ),
    (
        "patch1",
        "68d3e2d557594ae91bf4655c6a4fc9bde5a89cbe3958053cb61b0e09cd00183d",
    ),
    (
        "patch2",
        "8cc4f2403f4fe69faf1b77916d2db8ac3f713824dbfe273ba714c0a3fdf02d22",
    ),
    (
        "patch3",
        "28fd8c68edebdd8a014c40123261a2494ea3bde47089b3c2e4dd3cf474012a00",
    ),
    ("zig", "0.17.0"),
    ("optimize", "ReleaseSafe"),
    ("cpu", "baseline"),
    ("features", "upstream-default"),
    ("app-version", "1.4.1-dev"),
    ("lib-version", "1.0.1"),
    (
        "header-shellfox_extension.h",
        "a640920dbbecdb8759205250519d56978126c2dbf2524b585fd8b7eac346b763",
    ),
    (
        "header-shellfox_ansi.h",
        "4b8da0ec379a0dcfd5b4972b66981b4fed55019d70e304b17b5a4adeb94711dd",
    ),
    (
        "header-shellfox_terminal_profile.h",
        "06fb609354abaffcf5e95864c812d8658e6e78e5bf4cbf78376e03dc0dd36967",
    ),
    (
        "header-shellfox_snapshot_budget.h",
        "fa35639f1759a7804f6953cc68f467b3e664d33352bd8b6a5a756deb71a8a3a3",
    ),
    (
        "unicode",
        "2167c46bf05f2ecbb35a247f11216d44664b52c47c63bee86947823ddfa1fc90",
    ),
    ("terminal-profile", "native-only"),
];
fn text(path: &Path, limit: u64) -> Result<String> {
    println!("cargo:rerun-if-changed={}", path.display());
    let mut value = String::new();
    fs::File::open(path)?
        .take(limit + 1)
        .read_to_string(&mut value)?;
    if value.len() as u64 > limit {
        return Err(format!("oversize experimental metadata: {}", path.display()).into());
    }
    Ok(value)
}
fn digest(path: &Path) -> Result<String> {
    println!("cargo:rerun-if-changed={}", path.display());
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn fields(value: &str) -> Result<BTreeMap<&str, &str>> {
    if !value.ends_with('\n') || value.contains('\r') {
        return Err("experimental recipe needs canonical LF records".into());
    }
    let mut fields = BTreeMap::new();
    for line in value.lines() {
        let (key, value) = line
            .split_once(' ')
            .ok_or("malformed experimental recipe")?;
        if value.is_empty() || fields.insert(key, value).is_some() {
            return Err("duplicate/empty experimental recipe field".into());
        }
    }
    Ok(fields)
}

/// This checks consistency of trusted operator-selected build inputs, not
/// publisher authentication. The externally expected profile is compatibility.
pub fn validate(
    prefix: &Path,
    lib: &Path,
    target: &str,
    os: &str,
    expected: &str,
    project: &Path,
    out: &Path,
) -> Result<()> {
    let private = project.join("tmp").canonicalize()?;
    if !prefix.canonicalize()?.starts_with(&private) || !out.canonicalize()?.starts_with(&private) {
        return Err("experimental-shellfox-native requires installation and Cargo target-dir below project tmp; never overwrite loaded target/debug artifacts".into());
    }
    if lib.canonicalize()? != prefix.join("lib").canonicalize()? {
        return Err(
            "experimental-shellfox-native requires the prepared installation's own lib directory"
                .into(),
        );
    }
    if !sha(expected) {
        return Err("GHOSTTY_VT_ENGINE_PROFILE must be the expected lowercase SHA256 compatibility identity".into());
    }
    let recipe = text(&prefix.join(RECIPE), 8192).map_err(|e| format!("experimental Native002 preparation is required, not an official prebuilt installation: {e}"))?;
    if format!("{:x}", Sha256::digest(recipe.as_bytes())) != expected {
        return Err("experimental engine profile mismatch".into());
    }
    let values = fields(&recipe)?;
    if values.len() != FIXED.len() + 2 {
        return Err("unknown/missing experimental recipe fields".into());
    }
    for &(key, expected) in FIXED {
        if values.get(key).copied() != Some(expected) {
            return Err(format!("experimental recipe mismatch: {key}").into());
        }
    }
    if values.get("rust-target").copied() != Some(target) {
        return Err("experimental recipe Rust target mismatch".into());
    }
    if !values.get("zig-target").is_some_and(|value| {
        !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
    }) {
        return Err("invalid experimental Zig target".into());
    }
    let manifest = text(&prefix.join(FILES), 16384)?;
    let mut required = BTreeMap::new();
    for &(key, value) in FIXED.iter().filter(|(key, _)| key.starts_with("header-")) {
        required.insert(format!("include/ghostty/vt/{}", &key[7..]), Some(value));
    }
    for name in [
        "lib/pkgconfig/libghostty-vt.pc",
        "lib/pkgconfig/libghostty-vt-static.pc",
    ] {
        required.insert(name.into(), None);
    }
    let libraries: &[&str] = if os == "windows" {
        &["lib/ghostty-vt.lib", "bin/ghostty-vt.dll"]
    } else {
        &["lib/libghostty-vt.a"]
    };
    for &name in libraries {
        required.insert(name.into(), None);
    }
    let mut seen = BTreeMap::new();
    for line in BufReader::new(manifest.as_bytes()).lines() {
        let line = line?;
        let (hash, name) = line
            .split_once("  ")
            .ok_or("malformed experimental file manifest")?;
        if !sha(hash) || !required.contains_key(name) || seen.insert(name.to_owned(), ()).is_some()
        {
            return Err("unknown/duplicate/invalid experimental file record".into());
        }
        if required[name].is_some_and(|expected| expected != hash) {
            return Err(format!("experimental public header pin mismatch: {name}").into());
        }
        if digest(&prefix.join(name))? != hash {
            return Err(format!("experimental prepared input changed: {name}").into());
        }
    }
    if seen.len() != required.len() {
        return Err("incomplete experimental artifact/header manifest".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "experimental-install-tests.rs"]
mod tests;
