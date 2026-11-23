<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/nucelvo-logo-dark.svg">
    <img src="assets/brand/nucelvo-logo-light.svg" alt="Nucelvo" width="264">
  </picture>
</h1>

Free, open-source hardware inventory and live sensor monitoring for Windows and Linux. Built by **INformigados**. Pronounced “nu-CEL-vo.”

**Source preview.** Tested on Windows 11 x64. See the [compatibility notes](docs/compatibility.md) for Ubuntu and hardware coverage.

## 🖥️ What you can do

- Browse your CPU, memory, motherboard, graphics, storage, and system information. Search the inventory and copy individual values.
- Watch CPU and memory usage and any hardware sensors that your system exposes. Filter readings, pause collection, and view up to 15 minutes of history alongside session statistics.
- Preview and save reports as JSON, TXT, CSV, or standalone HTML. Share-safe reports omit serial numbers, volume names, and volume paths by default.
- Run offline without an account. The interface follows the system theme and opens in English. Select **Português (Brasil)** in the language menu to switch languages.

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
| [Changelog](CHANGELOG.md) | Notes for the initial source preview |
| [Contributing](CONTRIBUTING.md) | Development checks, bug reports, and pull requests |
| [Translations](translations/README.md) | Updating and reviewing language catalogs |

## 🇧🇷 Português (Brasil)

O Nucelvo é um aplicativo gratuito da INformigados para consultar o hardware e acompanhar sensores em tempo real. Ele funciona offline e permite pesquisar, copiar dados e exportar relatórios. Na primeira abertura, o aplicativo usa inglês. Selecione **Português (Brasil)** no menu de idioma para mudar a interface.

## 📄 License and credits

Nucelvo’s source code is licensed under [Apache 2.0](LICENSE). The desktop interface uses Slint under its Royalty-free License 2.0. See the [third-party notices](THIRD_PARTY_NOTICES.md) and [brand assets](assets/brand/README.md).

<a href="https://slint.dev/">
  <img src="assets/brand/made-with-slint.png" alt="Made with Slint" width="148">
</a>
