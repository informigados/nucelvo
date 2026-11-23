<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/nucelvo-logo-dark.svg">
    <img src="assets/brand/nucelvo-logo-light.svg" alt="Nucelvo" width="264">
  </picture>
</h1>

Free, open-source hardware inventory and live sensor monitoring for Windows and Linux. Built by **INformigados**. Pronounced “nu-CEL-vo.”

**Preview release.** Tested on Windows 11 x64. See the [compatibility notes](docs/compatibility.md) for Ubuntu and hardware coverage.

## ⬇️ Download for Windows

The [GitHub release](https://github.com/informigados/nucelvo/releases/tag/v0.5.0) includes a Windows x64 installer, a portable ZIP, and SHA-256 checksums. The installer runs for the current user. To use the portable edition, extract the ZIP and open `nucelvo.exe`.

## 🖥️ What you can do

- Browse your CPU, memory, motherboard, graphics, storage, and system information. Search the inventory and copy individual values.
- Check CPU, memory, graphics usage, and available temperatures in Overview. Open a reading to see its source, session statistics, and up to 15 minutes of history.
- On supported NVIDIA cards in Windows, monitor GPU temperature, usage, VRAM usage, fan speed, and power from the installed graphics driver. Sensor filters group readings by component or temperature.
- Check drive alerts, power-on hours, SSD endurance used, and other storage health readings when available. The System tab also shows power and battery information.
- Review physical network adapters, negotiated link speed, and cumulative traffic in System. On Windows, System also shows active displays and their current settings.
- Check for new GitHub releases on demand from About.
- Preview and save reports as JSON, TXT, CSV, or standalone HTML. Share-safe reports omit serial numbers, volume names, and volume paths by default.
- Run offline without an account. The interface opens in English and follows the system theme until you choose light or dark. The language menu also offers Brazilian Portuguese.

## 🚀 Run from source

Install [Rust with rustup](https://rustup.rs/). This project pins its toolchain in [rust-toolchain.toml](rust-toolchain.toml).

**Windows 11 x64** · Install the MSVC C++ build tools and Windows SDK.

**Ubuntu 24.04 x86_64** · Install the build and desktop dependencies.

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libfontconfig1-dev libxkbcommon-dev libxkbcommon-x11-0
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
