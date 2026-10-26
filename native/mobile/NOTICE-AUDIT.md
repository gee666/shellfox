# Mobile embedded dependency notices

Shellfox embeds Tailscale v1.104.2 under its BSD 3-Clause license, and golang.org/x/mobile under its BSD-style license. The native build generates THIRD-PARTY-NOTICES.txt and modules.json from the combined arm64 and amd64 dependency graphs for each target. It includes module-root license, notice and copyright files and the Go standard library license.

Android packages these files under assets/notices. The generated iOS pod includes them in ShellfoxNetworkNotices.bundle.

This collection is not a completed redistribution audit. Before release, review package-specific and vendored notices, generated assets, assembly sources, embedded data and licenses inherited from parent repositories. Review every entry in missingModuleRootLicenses. A successful build is not legal clearance.

This app embeds its own userspace Tailscale node. It does not bundle or require the Tailscale application, CLI, system daemon or OS VPN. Hosted coordination services and user accounts have separate service terms.
