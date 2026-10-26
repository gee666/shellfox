# Third-party notice audit

This helper embeds Tailscale v1.104.2 under its BSD 3-Clause license. The package includes `TAILSCALE-LICENSE` and a generated `THIRD-PARTY-NOTICES.txt` containing the license, notice and copyright files found at the roots of modules used by each target's compiled dependency graph. `modules.json` records their versions and identifies modules without a root license file. The Go standard library license is included too.

This collection is not a completed redistribution audit. Before publishing a release, review package-specific and vendored notices, generated assets, assembly sources, embedded data and licenses inherited from parent repositories. Check modules listed in `missingModuleRootLicenses`, and include any required notices the automated root-file collection misses. Tailscale's dependency graph differs across Windows, macOS and Linux, so review each target's output. Do not treat a successful build or an empty missing-license list as legal clearance.

The helper does not bundle the Tailscale desktop application, use its CLI, or install a system Tailscale daemon. The application uses Tailscale's hosted coordination service and user accounts; their service terms apply separately from the source license.
