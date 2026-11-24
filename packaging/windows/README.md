# Windows x64 packaging

The packaging script produces a per-user installer, a portable ZIP and
`SHA256SUMS` in `dist/windows-x64/<version>/`. These packages target native Windows x64.

## Build

Use Windows x64 with the workspace Rust toolchain, MSVC build tools and the
Windows SDK, Python 3.11 or newer, and NSIS 3. From the project root, run:

```powershell
python scripts/package-windows.py
```

For a portable NSIS installation, pass `--makensis` followed by the full path
to `makensis.exe`. Keep the NSIS distribution's `COPYING` file beside it.
The script builds the application in release mode with the static MSVC runtime,
runs a desktop smoke test and verifies the contents of the portable ZIP.

## Authenticode signing

To build signed packages, use a trusted code-signing certificate in the current
user's Personal certificate store and its provider's HTTPS RFC 3161 timestamp
endpoint. Set `$thumbprint` and `$timestampUrl` to those values before running
the command below. Run from a Windows SDK developer prompt or pass the path
to SignTool with `--signtool`.

```powershell
python scripts/package-windows.py --signing-thumbprint $thumbprint --timestamp-url $timestampUrl
```

The script signs and verifies the application executable before it packages
it, then signs and verifies the setup executable. It uses SHA-256 for both
signing and timestamping and generates checksums after signing. A signing or
verification failure stops the build before it replaces the output packages.
[Microsoft SignTool documentation](https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool)

## Install and remove

Open the setup executable and follow the English or Brazilian Portuguese
wizard. Setup defaults to `%LOCALAPPDATA%\Programs\Nucelvo` and adds a Start
menu and desktop shortcuts for the current Windows user. Administrator access is unnecessary.
Updates refresh managed shortcuts. Setup preserves an existing desktop shortcut with
the same name when it was created outside the installer.

To remove the app, open **Settings → Apps → Installed apps** and choose
**Uninstall** for Nucelvo. Preferences and exported reports are retained.

For portable use, extract the ZIP and open `nucelvo.exe`. Both editions use the
same preferences for the current Windows profile.

## Preview status

The preview is unsigned. Verify an artifact with
`Get-FileHash -Algorithm SHA256 <file>` and compare it with `SHA256SUMS`.
The packages include individual license files for the selected Windows
runtime dependencies and NSIS. See the [compatibility notes](../../docs/compatibility.md)
for the tested platform and hardware coverage.
