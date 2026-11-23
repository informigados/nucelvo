# Changelog

## 0.7.1 · Windows Update Preview

- Adds an opt-in update flow in About for installed Windows x64 copies. Nucelvo downloads the official setup, checks the exact size and SHA-256, and opens it through Windows Attachment Manager after confirmation.
- Keeps the GitHub release link available for portable copies, other platforms, and releases without a verifiable installer.
- Documents SmartScreen reputation prompts and Windows installer verification.

## 0.7.0 · Memory Diagnostics and Hardware Detail Preview

- Adds a Memory diagnostics section that displays the latest recorded Windows memory test result or the Linux EDAC error counters when available.
- Adds native Windows CPU cache totals and live clock readings for supported NVIDIA GPUs on Windows, AMD GPUs on Linux, and Linux CPUFreq policies.
- Adds a Frequencies sensor filter with chart and report support.
- Enlarges the author photos in About and preserves their original proportions.

## 0.6.0 · Live network monitoring preview

- Shows combined receive and send rates for connected Ethernet and Wi-Fi adapters on Windows and Linux.
- Adds network charts in System and a Network filter in Sensors. Both rates are available in sensor reports.
- Preserves gaps after pauses, adapter changes, and counter resets.
- Validates Ubuntu desktop smoke under X11 and Wayland and introduces initial macOS system, CPU, and memory collectors.

## 0.5.0 · About and system inventory preview

- Adds an About tab with author credits, the installed version, and a manual check for GitHub releases.
- Adds a desktop shortcut to the Windows installer and refreshes the Start menu shortcut.
- Shows active Windows displays, their current resolution, and refresh rate in System.
- Adds physical network adapter inventory with connection status, link speed, and cumulative traffic.

## 0.4.0 · Interface and navigation preview

- Adds distinct navigation and sensor icons, including a compact navigation rail.
- Adds action icons and persistent light and dark theme controls beside the language selector.
- Adds collapsible charts to inventory tabs with matching live readings.
- Groups verified volumes under their physical drive and adds a drive selector.
- Adds expandable explanations for inventory fields such as uptime, graphics memory, and storage health.

## 0.3.0 · Storage health preview

- Adds drive failure alerts, power-on hours, power cycles, and drive temperature when available.
- Adds estimated SSD endurance used, spare capacity, host reads and writes, unsafe shutdowns, and media errors for compatible NVMe drives.
- Reads ATA SMART on Windows and can use an installed smartctl on Linux. Verified ATA attribute decoding on two physical SATA hard drives.
- Adds power source, system battery detection, charge, and charging status to the System tab and inventory reports.
- Includes a Windows x64 installer, a portable ZIP, and SHA-256 checksums.

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
