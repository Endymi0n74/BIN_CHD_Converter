[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-0078d7.svg)](#macos-and-linux-cli)
[![.NET 10.0](https://img.shields.io/badge/.NET-10.0-512bd4.svg)](https://dotnet.microsoft.com/download/dotnet/10.0)
[![License: GPL v3](https://img.shields.io/badge/License-GPLv3-blue.svg)](LICENSE.txt)
[![GitHub release](https://img.shields.io/github/v/release/Endymi0n74/Batch_Convert_To_CHD_Next)](https://github.com/Endymi0n74/Batch_Convert_To_CHD_Next/releases)

# Batch Convert to CHD Next

> **Next desktop UI:** the lightweight Tauri/Rust client in `next-app/` runs on
> Windows, macOS and Linux without requiring .NET. It supports scanning,
> conversion, verification, automatic CD/DVD/HDD extraction, same-folder output,
> collision-safe renaming, archives, PBP, CCD and CSO/CISO. The legacy WPF client
> remains available for Windows users.

PBP and CCD support is provided by the `Next.FormatHelper` NativeAOT sidecar.
It is native code and does not require the .NET runtime on the destination PC.
GitHub Actions produces Windows x64, Linux x64/ARM64 and macOS Intel/Apple
Silicon packages from the same source tree.

Build the lightweight client:

```sh
cd next-app
npm install
npm run tauri build
```

**Batch Convert to CHD** is a high-performance Windows desktop utility designed to streamline the conversion of various disk image formats into the **Compressed Hunks of Data (CHD)** format.

![Batch Convert to CHD Screenshot](screenshot.png)
![Batch Convert to CHD Screenshot](screenshot2.png)
![Batch Convert to CHD Screenshot](screenshot3.png)

## 🚀 Key Features

### 💻 Modern Side-by-Side Dashboard
*   **Dual-Pane Interface**: View your settings and file list on the left, while monitoring real-time process logs on the right.
*   **Interactive File Selection**: Automatically scans folders and allows you to manually pick exactly which files to process via a detailed file list.
*   **Optimized File Loader**: Utilizes a chunked loading strategy to maintain UI responsiveness even when scanning directories with thousands of files.
*   **Resizable Layout**: Includes a built-in grid splitter to adjust the balance between the file explorer and the terminal view.

### 💻 Multi-Architecture Support
*   **Native ARM64 & x64**: Automatically detects your system architecture and utilizes the appropriate `chdman` binaries for conversion for maximum efficiency.
*   **Optimized Performance**: Leverages native instructions on ARM64 hardware to reduce overhead during heavy compression tasks.

### 🛠️ Intelligent Conversion & Extraction
*   **Automated Batch Processing**: Convert entire directories of disk images with real-time progress monitoring and immediate cancellation response.
*   **Recursive Structure Preservation**: Maintains your original directory hierarchy in the output folder when processing subfolders.
*   **Robust Extraction**: Supports extracting CHD files back to **.cue (CD)**, **.iso (DVD)**, **.gdi (Dreamcast/Naomi)**, and **.img (HDD)** with intelligent metadata auto-detection using the [CHDSharp](https://www.nuget.org/packages/CHDSharp) library.
*   **Archive Integration**: Transparently handles `.zip`, `.7z`, and `.rar` archives, extracting and processing contents automatically while respecting cancellation tokens. Includes a 7za.exe fallback for `.7z` files that SharpCompress cannot extract.
*   **CloneCD Support**: Convert CloneCD `.ccd` disc images to CHD format via the [CCDSharp](https://) library. Automatically generates CUE/BIN from `.ccd`/`.img` sets.
*   **CSO Decompression**: Built-in support for `.cso` and `.ciso` (Compressed ISO) files via the [CSOSharp](https://github.com/PureLogicCode/CSOSharp) library (supports deflate/zlib and LZ4).
*   **PBP Extraction**: Convert PlayStation Portable `.pbp` files to CHD format via the [PBPSharp](https://github.com/PureLogicCode/PBPSharp) library.

### ✅ Integrity, Safety & Verification
*   **Safe Deletion**: Source files (and their dependencies like `.bin`, `.sub`, etc.) are only deleted if the conversion/extraction is confirmed successful.
*   **Batch Verification**: Validate the checksums and structural integrity of existing CHD files using the [CHDSharp](https://www.nuget.org/packages/CHDSharp) library.
*   **Automated Organization**: Optionally move verified or failed files into dedicated subfolders (`Success`/`Failed`) while ignoring these special folders during subsequent scans.
*   **Cleanup**: Automatically removes empty subdirectories left behind after files are moved or deleted.
*   **Dependency Protection**: Performs a critical dependency check on startup to notify you if required components (like `chdman.exe`, needed for conversion) are missing.
*   **File System Monitoring**: Automatically monitors the input folder for file changes (deletions, renames, creations) during batch processing and provides diagnostic context when a file goes missing mid-operation.

### 📊 Performance & UI
*   **Real-time Telemetry**: Monitor disk write/read speeds and elapsed time during operations.
*   **Optimized Logging**: High-performance logging system with automatic truncation to keep the application responsive during long-running tasks.
*   **WPF-UI Theming**: Modern dark-themed UI powered by [WPF-UI](https://github.com/lepoco/wpfui) with Mica backdrop, rounded corners, and native Windows 11 aesthetics.

### 🔄 Updates & Stability
*   **Automatic Update Checks**: Notifies you immediately if a newer version is available on GitHub at startup.
*   **Automated Bug Reporting**: Built-in error reporting system helps improve the application by automatically sending crash reports (no personal data collected).

---

## 📂 Supported Formats

| Category             | Formats                                                    |
|:---------------------|:-----------------------------------------------------------|
| **Standard Images**  | `.iso`, `.cue` (+`.bin`), `.img`, `.ccd` (+`.img`), `.raw`, `.toc` |
| **Console Specific** | `.gdi` (Dreamcast), `.pbp` (PlayStation)                   |
| **Compressed**       | `.cso` (Compressed ISO)                                    |
| **Archives**         | `.zip`, `.7z`, `.rar`                                      |
| **Output**           | `.chd` (Compressed Hunks of Data)                          |

---

## 🛠️ Technical Logic

The application implements priority-based logic to ensure compatibility:

1.  **DVD Images (`.iso`)**: Defaults to `createdvd`.
2.  **Multi-track Images (`.cue`, `.gdi`, `.toc`)**: Defaults to `createcd`.
3.  **Hard Disk Images (`.img`)**: Defaults to `createhd` unless an accompanying `.cue` file is detected, in which case `createcd` is used.
4.  **Raw Data (`.raw`)**: Defaults to `createraw`.
5.  **PlayStation PBP (`.pbp`)**: Extracts to CUE/BIN using PBPSharp, then converts to CHD using `createcd`.
6.  **CloneCD (`.ccd`)**: Converts to CUE/BIN using CCDSharp, then converts to CHD using `createcd`.

*Note: Users can manually override these settings via the UI to force specific modes (except for PBP which always extracts first).*

---

## 💻 Requirements

*   **Operating System**: Windows 10 / 11 (x64 or ARM64)
*   **Runtime**: [.NET 10.0 Desktop Runtime](https://dotnet.microsoft.com/download/dotnet/10.0)
*   **Bundled Dependencies**:
    *   `chdman.exe` / `chdman_arm64.exe` (MAME Project — conversion only)
    *   `7za.exe` / `7za_arm64.exe` (7-Zip fallback extraction)
*   **Library Dependencies**:
    * [WPF-UI](https://github.com/lepoco/wpfui) (v4.3.0) — Modern Fluent Design theming and controls
    * [CHDSharp](https://www.nuget.org/packages/CHDSharp) (v1.2.0) — Pure C# CHD reading, verification, and extraction
    * [CSOSharp](https://) (v1.0.0) — Pure C# CSO/CISO decompression (deflate + LZ4)
    * [PBPSharp](https://) (v1.0.0) — Pure C# PBP extraction and SFO parsing
    * [CCDSharp](https://) (v1.0.0) — Pure C# CloneCD (.ccd/.img/.sub) parsing and conversion
    * [SharpCompress](https://github.com/adamhathcock/sharpcompress) (v0.50.1) — Archive extraction support
    * [Serilog](https://serilog.net/) (v4.4.0) — Structured diagnostic logging

---

## 📥 Installation

1.  Download the portable archive for your processor (`win-x64` or `win-arm64`) from the [Releases](https://github.com/Endymi0n74/Batch_Convert_To_CHD_Next/releases) page.
2.  Extract the contents to a permanent folder.
3.  **Important**: Ensure all `.exe` files (including ARM64 variants) remain in the same directory as `BatchConvertToCHD.exe`.
4.  Launch the application.

The portable Windows builds are self-contained: **the .NET runtime does not
need to be installed** on the destination PC. Each archive only contains the
`chdman` and `7za` binaries needed by its target architecture.

---

## 📖 Usage

The application also accepts a folder path as a command-line argument to quickly populate the source directory:
```sh
BatchConvertToCHD.exe "C:\ROMs\MyGames"
```

### Conversion Workflow
1.  Navigate to the **Convert to CHD** tab.
2.  Select your **Source Folder** (containing images or archives).
3.  Select your **Output Folder**.
4.  *(Optional)* Check "Process smaller files first" to sort by file size.
5.  *(Optional)* Check "Force CD" or "Force DVD" to override automatic command detection.
6.  *(Optional)* Set a time limit per file to abort conversions that exceed the specified duration.
7.  *(Optional)* Enable "Delete original files" to clean up source data after a successful conversion.
8.  Click **Start Conversion**.

### Extraction Workflow
1.  Navigate to the **Extract CHD Files** tab.
2.  Select your **Source Folder** (containing `.chd` files).
3.  Select your **Output Folder**.
4.  Choose the desired output format (Auto-detect, CD `.cue`, DVD `.iso`, Dreamcast `.gdi`, HDD `.img`).
5.  *(Optional)* Enable "Include subfolders" to process nested directories.
6.  *(Optional)* Enable "Delete original CHD files" to clean up after successful extraction.
7.  Click **Start Extraction**.

### Verification Workflow
1.  Navigate to the **Verify CHD Files** tab.
2.  Select the folder containing your `.chd` files.
3.  Configure folder organization options (Success/Failed folders).
4.  Click **Start Verification**.

---

## 🤝 Contributing & Support

If you encounter issues or have feature requests, please use the [GitHub Issues](https://github.com/Endymi0n74/Batch_Convert_To_CHD_Next/issues) tracker.

**Support the Project:**
If this tool saves you time, consider supporting further development:
*   ⭐ **Star this repository** on GitHub.
*   ☕ **Donate**: [www.purelogiccode.com/donate](https://www.purelogiccode.com/donate)

---

## 📜 License

This project is licensed under the **GNU General Public License v3.0**. See the [LICENSE.txt](LICENSE.txt) file for details.

**Acknowledgements:**
*   [MAME Team](https://www.mamedev.org/) for `chdman`.
*   [CHDSharp](https://www.nuget.org/packages/CHDSharp) by Peterson Fernandes — Pure C# CHD read-only library supporting V1-V5, all 10 codecs, parent/child chaining, and parallel verification.
*   [WPF-UI](https://github.com/lepoco/wpfui) by lepoco — Modern Windows 11 Fluent Design theming and controls.
*   [CSOSharp](https://) by Peterson Fernandes — Pure C# CSO/CISO decompression library.
*   [PBPSharp](https://) by Peterson Fernandes — Pure C# PlayStation PBP extraction library.
*   [CCDSharp](https://) by Peterson Fernandes — Pure C# CloneCD disc image parsing and conversion library.
*   [SharpCompress](https://github.com/adamhathcock/sharpcompress) for archive handling.
*   [Serilog](https://serilog.net/) for structured logging.
*   [Igor Pavlov](https://www.7-zip.org/) for `7za.exe` (7-Zip command-line tool).

---
Developed by [Pure Logic Code](https://www.purelogiccode.com)

## macOS and Linux CLI

The `batchconverttochd` frontend supports batch conversion, extraction and
verification on macOS/Linux, including recursive traversal, parallel jobs,
tree preservation, dry runs and safe source deletion. Install `chdman` first
(`brew install mame` on macOS or your distribution's `mame-tools` package), then:

```sh
chmod +x batchconverttochd.sh install-unix.sh
sudo ./install-unix.sh               # system-wide
./install-unix.sh --user             # or only for the current user
batchconverttochd convert ~/ROMs ~/CHD -r -j 4
batchconverttochd extract ~/CHD ~/Extracted -r --format auto
batchconverttochd verify ~/CHD -r
```

The WPF graphical interface remains Windows-only; run
`batchconverttochd --help` for all cross-platform CLI options.
