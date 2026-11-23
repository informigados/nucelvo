# Changelog

## 0.2.0 · Windows installer preview

- Restores the full window on Windows after minimizing, including when the window was maximized.
- Adds live CPU and memory usage, GPU usage, and available temperature readings to Overview.
- Groups inventory fields by device and adds component and temperature filters to Sensors. Select a live reading to open its history.
- Adds NVIDIA GPU temperature, usage, VRAM usage, fan speed, and power on Windows where the installed driver exposes them.
- Includes a per-user Windows 11 x64 installer, a portable ZIP, and SHA-256 checksums. The preview binaries are unsigned.

## 0.1.0 · Initial source preview

### Included

- An offline hardware inventory with six categories, search, and per-field copying.
- Live CPU and memory usage, per-logical-processor readings where available, session statistics, and up to 15 minutes of sensor history.
- Report previews and JSON, TXT, CSV, and standalone HTML exports. Share-safe filtering is enabled by default.
- English and Brazilian Portuguese, with a saved language preference and system theme support.

### Compatibility

- Windows 11 x64 was exercised on physical hardware with a standard user account.
- Ubuntu 24.04 LTS x86_64 is the Linux target. Cross-checks passed. Native Ubuntu compatibility has yet to be verified.
- Sensor availability depends on the operating system, hardware, firmware, drivers, and permissions.

See the [compatibility notes](docs/compatibility.md) for platform and hardware coverage.
