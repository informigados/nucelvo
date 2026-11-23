<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/nucelvo-logo-dark.svg">
    <img src="assets/brand/nucelvo-logo-light.svg" alt="Nucelvo" width="264">
  </picture>
</h1>

Free, open-source hardware inventory and live sensor monitoring for Windows and Linux. Built by **INformigados**. Pronounced “nu-CEL-vo.”

**Preview release.** Tested on Windows 11 x64. See the [compatibility notes](docs/compatibility.md) for Ubuntu and hardware coverage.

## ⬇️ Download for Windows

The [GitHub release](https://github.com/informigados/nucelvo/releases/tag/v0.7.1) includes a Windows x64 installer, a portable ZIP, and SHA-256 checksums. The installer runs for the current user. To use the portable edition, extract the ZIP and open `nucelvo.exe`.

### 🔍 Verify the Windows installer

Download the installer from the [official GitHub release](https://github.com/informigados/nucelvo/releases/tag/v0.7.1). In PowerShell, run this command from the download folder and compare the output with `SHA256SUMS` attached to the release.

```powershell
Get-FileHash .\nucelvo-0.7.1-windows-x64-setup.exe -Algorithm SHA256
```

For the **0.7.1 Windows x64 installer**, the published SHA-256 is `609abee271e4df8f9aab031c054b3a9f927f5e2079497aa31e346a3188a60f1b`.

### 🛡️ Third-party analysis

| Service | Report |
| --- | --- |
| VirusTotal | The recorded scan showed 2 detections, 65 engines with no detection, and 3 engines that did not support the file type. The detections remain unconfirmed. [View VirusTotal report](https://www.virustotal.com/gui/file/609abee271e4df8f9aab031c054b3a9f927f5e2079497aa31e346a3188a60f1b) |
| Hybrid Analysis | The sample summary showed a threat score of 0/100, 0 of 27 antivirus engines flagging the file, and 11 suspicious behavioral indicators. [View Hybrid Analysis report](https://hybrid-analysis.com/sample/609abee271e4df8f9aab031c054b3a9f927f5e2079497aa31e346a3188a60f1b) |
| MetaDefender Cloud | MetaScan flagged the installer in 1 of 21 engines. Webroot SMD reported `Malware_51.4`, and Adaptive Sandbox rated the installer **Suspicious**. The archive view also flagged the bundled NSIS language-selection plug-in, `LangDLL.dll`. [View MetaDefender Cloud report](https://metadefender.com/results/file/bzI2MTEyMzVpaVpJdXQ2dHBiUGNQT2RiZ0xI_mdaas/overview) |

These services use different engines, rules, and test environments, and results can change over time. A checksum confirms file identity. Scan results describe what a service observed for that file.

Nucelvo Setup installs files for the current user, creates an uninstaller and shortcuts, and adds an entry to Windows Installed Apps. These operations account for some of the observed file and registry activity. Other indicators require individual review. [Windows installer source](packaging/windows/Nucelvo.nsi)

### Microsoft Defender SmartScreen

The Windows installer is unsigned. Microsoft Edge may show a reputation warning if the file is uncommon or unrecognized. If you choose to keep the official installer after checking its source and SHA-256, open the download item's **…** menu, select **Keep**, and select **Keep anyway** under **Show more** if that option is available. Leave SmartScreen enabled and follow any malware warning or organization policy. [Microsoft Edge SmartScreen documentation](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-security-smartscreen)

## 🖥️ What you can do

- Browse your CPU, memory, motherboard, graphics, storage, and system information. Search the inventory and copy individual values.
- Review the latest recorded Windows memory test or the Linux EDAC error counters in Memory when the system exposes them.
- Check CPU, memory, graphics usage, and available temperatures in Overview. Open a reading to see its source, session statistics, and up to 15 minutes of history.
- On supported NVIDIA cards in Windows, monitor GPU temperature, usage, VRAM usage, fan speed, power, and clock frequencies from the installed driver. Use the Frequencies filter to find supported CPU and GPU clocks.
- Check drive alerts, power-on hours, SSD endurance used, and other storage health readings when available. The System tab also shows power and battery information.
- Review physical network adapters, negotiated link speed, and cumulative traffic in System. On Windows, System also shows active displays and their current settings.
- Check for new releases from About. On installed Windows x64 copies, select **Update now** to download and verify an eligible installer, then select **Install update and close Nucelvo** to open the installer.
- See live receive and send rates under System. Select a rate to open its history, or choose Network in Sensors to view and export readings.
- Preview and save reports as JSON, TXT, CSV, or standalone HTML. Share-safe reports omit serial numbers, volume names, and volume paths by default.
- Run offline without an account. The interface opens in English and follows the system theme until you choose light or dark. The language menu also offers Brazilian Portuguese.

## 🚀 Run from source

Install [Rust with rustup](https://rustup.rs/). This project pins its toolchain in [rust-toolchain.toml](rust-toolchain.toml).

**Windows 11 x64** · Install the MSVC C++ build tools and Windows SDK.

**Ubuntu 24.04 x86_64** · Install the build and desktop dependencies.

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libfontconfig1-dev libxkbcommon-dev \
  libxkbcommon-x11-0 libx11-xcb1 libxcursor1 libxi6
```

The Linux file chooser uses the desktop portal, with Zenity as a fallback. Install `zenity` if your desktop lacks a file chooser portal and `xdg-utils` to use **Open folder**.

From the project root, run:

```sh
cargo run --locked
```

## 📚 Project resources

| Resource | What it covers |
| --- | --- |
| [Compatibility](docs/compatibility.md) | Platform support and hardware coverage |
| [Changelog](CHANGELOG.md) | Changes by release |
| [Contributing](CONTRIBUTING.md) | Development checks, bug reports, and pull requests |
| [Security policy](SECURITY.md) | How to report a vulnerability privately |
| [Translations](translations/README.md) | Updating and reviewing language catalogs |

## 📄 License and credits

Nucelvo’s source code is licensed under [Apache 2.0](LICENSE). The desktop interface uses Slint under its Royalty-free License 2.0. See the [third-party notices](THIRD_PARTY_NOTICES.md) and [brand assets](assets/brand/README.md).

<a href="https://slint.dev/">
  <img src="assets/brand/made-with-slint.png" alt="Made with Slint" width="148">
</a>
