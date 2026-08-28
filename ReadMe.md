# BIN_CHD_Converter

Portable desktop batch converter for optical-disc and hard-disk images.

[![Build](https://github.com/Endymi0n74/BIN_CHD_Converter/actions/workflows/next-release.yml/badge.svg)](https://github.com/Endymi0n74/BIN_CHD_Converter/actions/workflows/next-release.yml)
[![Latest release](https://img.shields.io/github/v/release/Endymi0n74/BIN_CHD_Converter)](https://github.com/Endymi0n74/BIN_CHD_Converter/releases/latest)
[![License](https://img.shields.io/badge/license-GPL--3.0-blue.svg)](LICENSE.txt)

## What it does

BIN_CHD_Converter converts disc images to [CHD](https://docs.mamedev.org/tools/chdman.html) and extracts CHD files back to usable images. The modern client is built with Tauri/Rust and uses native sidecars, so the portable archives do not require the .NET runtime.

- Batch conversion with recursive folder scanning.
- Real-time progress, logs, ETA and cancellation.
- Automatic CD/DVD/HDD routing based on file type and image content.
- CHD extraction to BIN/CUE, ISO or IMG.
- Collision-safe extraction: existing files are never overwritten.
- Safe staging for non-ASCII and overlong Windows paths.
- Sector-alignment preflight with clear skip messages.
- Portable archives for Windows, macOS and Linux.

## Supported inputs

| Type | Extensions | Notes |
|---|---|---|
| CD / GD-ROM | `.cue`, `.bin`, `.raw`, `.ccd`, `.mds`, `.ecm` | Multi-track and raw-sector images are supported where their companion files are available. |
| DVD | `.iso` | Routed to `createdvd`. |
| Hard disk | `.img` | Routed to `createhd` unless content detection identifies a raw CD. |
| Console | `.gdi`, `.pbp` | Dreamcast and PlayStation images. |
| Compressed | `.cso`, `.ciso` | Decoded through the native format helper. |
| Archives | `.zip`, `.7z`, `.rar` | Archives are unpacked to a temporary workspace. |
| Output | `.chd` | CHD extraction supports CD, DVD and HDD targets. |

ECM and MDS conversion is provided by the `batch-format-helper` NativeAOT sidecar. MDS sets must keep their `.mdf` and split data files beside the `.mds` descriptor.

## Download

Download the latest portable archive from the [Releases page](https://github.com/Endymi0n74/BIN_CHD_Converter/releases/latest):

- `BIN_CHD_Converter-win-x64-portable.zip`
- `BIN_CHD_Converter-win-arm64-portable.zip`
- `BIN_CHD_Converter-osx-x64-portable.tar.gz`
- `BIN_CHD_Converter-osx-arm64-portable.tar.gz`
- `BIN_CHD_Converter-linux-x64-portable.tar.gz`
- `BIN_CHD_Converter-linux-arm64-portable.tar.gz`

The macOS and Linux builds are currently published as best-effort, untested portable artifacts. They are not installer packages. On Unix systems, make the application executable before launching it:

```sh
chmod +x BIN_CHD_Converter batch-format-helper-*
./BIN_CHD_Converter
```

Each portable archive includes a matching native `chdman` executable from MAME alongside `BIN_CHD_Converter` and `batch-format-helper`. The application searches its own directory first, then falls back to `chdman` on `PATH`. You do not need to install MAME separately for the published archives.

## Windows portable build

The Windows archives are self-contained and do not require the .NET runtime. Extract one archive, keep its files together, and launch `BIN_CHD_Converter.exe`.

The repository also retains the legacy WPF client under `BatchConvertToCHD/`; the Tauri client is the recommended cross-platform application.

## Build from source

Requirements:

- .NET SDK 10
- Node.js 24 and npm
- Rust stable and Cargo
- Tauri 2 build prerequisites for the target platform
- MAME `chdman` only when running conversion tests locally; published archives already include it

Build and test the .NET components:

```sh
dotnet test BatchConvertToCHD.Tests/BatchConvertToCHD.Tests.csproj
```

Build the frontend and Rust client:

```sh
cd next-app
npm ci
npm run build
cargo build --release --manifest-path src-tauri/Cargo.toml
```

Build the NativeAOT format helper:

```sh
dotnet publish Next.FormatHelper/Next.FormatHelper.csproj -c Release -r win-x64 --self-contained true
```

## Command-line format helper

The sidecar can be invoked directly:

```text
batch-format-helper <pbp|ccd|cso|ecm|mds> <input> <output-directory>
```

It prints the generated convertible file path to standard output and diagnostics to standard error.

## Safety notes

- Source files are deleted only after successful conversion when deletion is enabled.
- Partial outputs are removed after a failed or canceled `chdman` operation.
- Extraction diverts to a numbered subdirectory when the destination already exists.
- Images with invalid sector alignment are skipped before conversion.
- macOS/Linux artifacts have not been tested locally; contributions and reports are welcome.

## Contributing and support

Please open an issue at [GitHub Issues](https://github.com/Endymi0n74/BIN_CHD_Converter/issues) with the operating system, architecture, input format and relevant log output.

## License

BIN_CHD_Converter is distributed under the GNU General Public License v3.0. See [LICENSE.txt](LICENSE.txt).

The project uses or incorporates CHDSharp, CSOSharp, PBPSharp, CCDSharp, SharpCompress, Tauri and MAME/chdman. See the source and package metadata for their respective licenses and notices.
