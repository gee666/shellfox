use crate::{digest::parse_hex, manifest::*, Error, Result};
use std::collections::BTreeSet;

pub(crate) fn relative_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 512 || path.split('/').count() > 16 {
        return Err(Error::Manifest("invalid relative path length"));
    }
    for part in path.split('/') {
        let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit());
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > 128
            || part.ends_with('.')
            || reserved
            || !part
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        {
            return Err(Error::Manifest("ambiguous or escaping relative path"));
        }
    }
    Ok(())
}

pub(crate) fn validate(manifest: &BundleManifest) -> Result<()> {
    if manifest.format != "shellfox-native-bundle-v1"
        || manifest.identity.version.is_empty()
        || manifest.identity.version.len() > 64
        || !manifest
            .identity
            .version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
    {
        return Err(Error::Manifest("unknown format or invalid bundle version"));
    }
    if manifest.files.0.is_empty()
        || manifest.files.0.len() > MAX_FILES
        || manifest.entrypoints.is_empty()
        || manifest.entrypoints.len() > 8
    {
        return Err(Error::Manifest("invalid file or entrypoint bound"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for file in &manifest.files.0 {
        relative_path(&file.path)?;
        parse_hex(&file.sha256)?;
        if file
            .path
            .to_ascii_lowercase()
            .ends_with(".runtimeconfig.dev.json")
        {
            return Err(Error::Manifest(
                "development loader probing configuration refused",
            ));
        }
        if file.path.eq_ignore_ascii_case(MANIFEST_NAME)
            || !names.insert(file.path.to_ascii_lowercase())
            || file.size > MAX_FILE
        {
            return Err(Error::Manifest("duplicate, reserved or oversized payload"));
        }
        total = total
            .checked_add(file.size)
            .ok_or(Error::Manifest("bundle size overflow"))?;
    }
    if total > MAX_TOTAL {
        return Err(Error::Manifest("bundle too large"));
    }
    let mut entry_names = BTreeSet::new();
    let mut protocols = BTreeSet::new();
    for entry in &manifest.entrypoints {
        relative_path(&entry.path)?;
        if !manifest.files.0.iter().any(|file| file.path == entry.path)
            || !entry_names.insert(&entry.path)
            || entry.protocols.is_empty()
            || entry.protocols.len() > 8
        {
            return Err(Error::Manifest("invalid entrypoint file or protocol count"));
        }
        for contract in &entry.protocols {
            let required = capabilities(contract.id, manifest.identity.rid);
            if contract.version != 1
                || contract.capabilities.len() != required.len()
                || contract.capability_set() != required
                || !protocols.insert(contract.id)
            {
                return Err(Error::Manifest(
                    "unsupported or duplicate protocol/capability contract",
                ));
            }
        }
    }
    use ProtocolId::*;
    use RuntimeId::*;
    let windows = matches!(manifest.identity.rid, WinX64 | WinArm64);
    let osx = matches!(manifest.identity.rid, OsxX64 | OsxArm64);
    let expected: BTreeSet<_> = match (&manifest.identity.id, &manifest.runtime) {
        (
            TrustedBundleId::WindowsManagedHelper,
            ResourceRuntime::Dotnet {
                version,
                target_framework,
                entry_assembly,
                self_contained,
            },
        ) if windows => {
            let framework = dotnet_framework(version)?;
            if !*self_contained
                || target_framework != &format!("net{framework}-windows")
                || manifest.entrypoints.len() != 1
                || entry_assembly.contains('/')
            {
                return Err(Error::Manifest(
                    "expected a self-contained Windows helper with matching audited .NET framework",
                ));
            }
            let entry = &manifest.entrypoints[0].path;
            let stem = entry
                .strip_suffix(".exe")
                .ok_or(Error::Manifest("managed apphost must be .exe"))?;
            if entry.contains('/') || entry_assembly != &format!("{stem}.dll") {
                return Err(Error::Manifest("apphost/assembly mismatch"));
            }
            for required in [
                format!("{stem}.dll"),
                format!("{stem}.deps.json"),
                format!("{stem}.runtimeconfig.json"),
                "hostfxr.dll".into(),
                "hostpolicy.dll".into(),
                "coreclr.dll".into(),
                "clrjit.dll".into(),
                "System.Private.CoreLib.dll".into(),
            ] {
                if !manifest.files.0.iter().any(|file| file.path == required) {
                    return Err(Error::Manifest("missing managed runtime payload"));
                }
            }
            [ElevatedConpty, ElevatedIsolated, PowerGuardian]
                .into_iter()
                .collect()
        }
        (TrustedBundleId::GoTailscale, ResourceRuntime::Go { version })
            if version_number(version) =>
        {
            [Tailscale].into_iter().collect()
        }
        (TrustedBundleId::DarwinTerminal, ResourceRuntime::Native { abi })
            if osx && abi == "darwin-libproc-v1" =>
        {
            [DarwinSnapshot, DarwinSupervisor].into_iter().collect()
        }
        (TrustedBundleId::WslSupervisor, ResourceRuntime::Python { major: 3 })
            if matches!(manifest.identity.rid, LinuxX64 | LinuxArm64) =>
        {
            for required in ["guardian.py", "bootstrap.py", "tree.py", "wire.py"] {
                if !manifest.files.0.iter().any(|file| file.path == required) {
                    return Err(Error::Manifest("missing WSL module"));
                }
            }
            [WslGuardian].into_iter().collect()
        }
        _ => return Err(Error::Manifest("runtime, RID and resource kind disagree")),
    };
    if protocols != expected {
        return Err(Error::Manifest("resource protocol set differs"));
    }
    Ok(())
}

pub(crate) fn version_number(value: &str) -> bool {
    value.len() <= 32
        && value.split('.').count() == 3
        && value.split('.').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Metadata compatibility, not runtime security or lifecycle clearance. Keep
/// this closed to audited framework layouts rather than accepting future TFMs.
pub(crate) fn dotnet_framework(version: &str) -> Result<&'static str> {
    if !version_number(version) {
        return Err(Error::Manifest("exact .NET runtime version required"));
    }
    let mut parts = version.split('.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(major), Some("0"), Some(patch)) if patch == "0" || !patch.starts_with('0') => {
            match major {
                "8" => Ok("8.0"),
                "9" => Ok("9.0"),
                "10" => Ok("10.0"),
                _ => Err(Error::Manifest("unsupported .NET framework layout")),
            }
        }
        _ => Err(Error::Manifest(
            "noncanonical or unsupported .NET framework version",
        )),
    }
}

pub fn capabilities(id: ProtocolId, rid: RuntimeId) -> BTreeSet<Capability> {
    use Capability::*;
    use ProtocolId::*;
    let values: &[Capability] = match id {
        ElevatedConpty => &[Conpty],
        ElevatedIsolated => &[IsolatedWindowsPowerShell, NoProfile],
        PowerGuardian => &[Display, System, AwayMode, IdleMouse, LidRollback],
        Tailscale if matches!(rid, RuntimeId::WinX64 | RuntimeId::WinArm64) => {
            &[EmbeddedNetwork, Ownership, PrivatePermissions]
        }
        Tailscale => &[EmbeddedNetwork, Ownership],
        DarwinSnapshot => &[
            ExactBirth,
            IdentityAccess,
            Enumeration,
            Argv,
            EnvironmentMarker,
        ],
        DarwinSupervisor => &[OwnedSession, Guardians, Termination],
        WslGuardian => &[Pidfd, Subreaper, Peercred],
    };
    values.iter().copied().collect()
}
