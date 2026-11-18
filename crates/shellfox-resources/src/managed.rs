use crate::{
    contracts::{dotnet_framework, relative_path},
    manifest::ResourceRuntime,
    tree::HeldTree,
    BundleManifest, Error, Result,
};
use serde_json::Value;

pub(crate) fn validate_runtime(manifest: &BundleManifest, tree: &HeldTree) -> Result<()> {
    let ResourceRuntime::Dotnet {
        version,
        entry_assembly,
        ..
    } = &manifest.runtime
    else {
        return Ok(());
    };
    let framework = dotnet_framework(version)?;
    let tfm = format!("net{framework}");
    let stem = entry_assembly
        .strip_suffix(".dll")
        .ok_or(Error::Manifest("managed assembly suffix"))?;
    let config: Value = serde_json::from_slice(
        &tree.read_verified(&format!("{stem}.runtimeconfig.json"), 1024 * 1024)?,
    )
    .map_err(|_| Error::Manifest("invalid runtime configuration JSON"))?;
    let options = &config["runtimeOptions"];
    let frameworks = options["includedFrameworks"]
        .as_array()
        .ok_or(Error::Manifest("self-contained framework list missing"))?;
    if options["tfm"].as_str() != Some(tfm.as_str())
        || frameworks.len() != 1
        || frameworks[0]["name"] != "Microsoft.NETCore.App"
        || frameworks[0]["version"].as_str() != Some(version)
        || options.get("framework").is_some()
        || options.get("frameworks").is_some()
        || options.get("additionalProbingPaths").is_some()
    {
        return Err(Error::Manifest(
            "runtime configuration does not bind the declared self-contained version",
        ));
    }
    let deps: Value =
        serde_json::from_slice(&tree.read_verified(&format!("{stem}.deps.json"), 4 * 1024 * 1024)?)
            .map_err(|_| Error::Manifest("invalid dependency JSON"))?;
    let rid = manifest.identity.rid.as_str();
    let target = format!(".NETCoreApp,Version=v{framework}/{rid}");
    if deps["runtimeTarget"]["name"].as_str() != Some(&target) {
        return Err(Error::Manifest("dependency RID mismatch"));
    }
    let libraries = deps["targets"][&target]
        .as_object()
        .ok_or(Error::Manifest("dependency target missing"))?;
    let runtimepack = format!("runtimepack.Microsoft.NETCore.App.Runtime.{rid}/{version}");
    if !libraries.contains_key(&runtimepack) {
        return Err(Error::Manifest("declared runtime pack missing"));
    }
    for library in libraries.values() {
        for category in ["runtime", "native", "resources", "runtimeTargets"] {
            if let Some(assets) = library.get(category) {
                let assets = assets
                    .as_object()
                    .ok_or(Error::Manifest("invalid runtime asset table"))?;
                for asset in assets.keys() {
                    relative_path(asset)?;
                    if asset.ends_with("/_._") {
                        continue;
                    }
                    let basename = asset
                        .rsplit('/')
                        .next()
                        .ok_or(Error::Manifest("missing asset basename"))?;
                    if !manifest
                        .files
                        .0
                        .iter()
                        .any(|file| file.path == *asset || file.path == basename)
                    {
                        return Err(Error::Manifest(
                            "dependency asset missing from complete publish file set",
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
