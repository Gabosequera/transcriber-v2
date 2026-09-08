# Third-party software and fonts

Transcriptor V2 original code is proprietary, Copyright2026 Gabriel Sequera. Third-party components remain under their respective licenses; LICENSE.md does not restrict those grants.

The release includes `third-party/rust/inventory.json` and license/copyright notices for the resolved Rust dependencies. This conservative inventory includes development/optional packages, not only linked code. Supplemental notices in `packaging/third-party/rust-notices` were obtained from the exact source commits recorded by the published crates in .cargo_vcs_info.json; PROVENANCE.json records URLs and SHA256. Font licenses OFL1.1 and Ubuntu Font Licence1.0 are included with epaint_default_fonts.

## FFmpeg / FFprobe

Public editor ZIPs do not contain FFmpeg binaries. FFmpeg/FFprobe are independent executables used for media probing, decoding and encoding. Install-FFmpeg.ps1 optionally downloads Gyan's8.0.1 essentials build directly from its official GitHub distribution and checks the expected SHA256 of both executables before installation. It preserves the upstream GPLv3 license and README. You may instead supply an existing compatible installation through TRANSCRIPTOR_FFMPEG_DIR.

Distributor: https://github.com/GyanD/codexffmpeg/releases/tag/8.0.1
Project/legal information: https://ffmpeg.org/legal.html
Source reference for the tested upstream build: https://github.com/FFmpeg/FFmpeg/commit/894da5ca7d
The upstream binaries and their dependencies remain GPLv3/other applicable third-party licenses. They are not relicensed as proprietary software by this repository. Historic local development bundles containing these binaries are not the public editor release assets.
