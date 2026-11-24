"""Build a local Windows x64 release, installer, portable archive and checksums."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
import tomllib
from urllib.parse import urlsplit
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGET = "x86_64-pc-windows-msvc"
OUTPUT_ROOT = ROOT / "dist" / "windows-x64"
PAYLOAD = ("nucelvo.exe", "LICENSE", "THIRD_PARTY_NOTICES.md", "THIRD_PARTY_LICENSES.txt")
SUPPLEMENTS = ROOT / "packaging" / "windows" / "licenses"


def find_makensis(explicit):
    if explicit:
        location = Path(explicit)
    else:
        installed = shutil.which("makensis")
        location = Path(installed) if installed else Path(os.environ.get("ProgramFiles(x86)", r"C:\Program Files (x86)")) / "NSIS" / "makensis.exe"
    if not location.is_file():
        raise SystemExit("Install NSIS 3 or pass --makensis with the path to makensis.exe.")
    return location.resolve()


def run(args, **kwargs):
    return subprocess.run(args, cwd=ROOT, check=True, **kwargs)


def signing_options(args, parser):
    if not any((args.signing_thumbprint, args.timestamp_url, args.signtool)):
        return None
    if not args.signing_thumbprint or not re.fullmatch(r"[0-9a-fA-F]{40}", args.signing_thumbprint):
        parser.error("--signing-thumbprint must be a 40-digit certificate thumbprint")
    try:
        timestamp = urlsplit(args.timestamp_url or "")
        if (
            timestamp.scheme != "https"
            or not timestamp.hostname
            or timestamp.username is not None
            or timestamp.fragment
        ):
            parser.error("--timestamp-url must be an HTTPS RFC 3161 endpoint")
    except ValueError:
        parser.error("--timestamp-url must be a valid HTTPS URL")
    tool = shutil.which(args.signtool or "signtool.exe")
    if not tool:
        parser.error("Run from a Windows SDK developer prompt or provide --signtool")
    return Path(tool).resolve(), args.signing_thumbprint, args.timestamp_url


def sign_file(path, signing):
    tool, thumbprint, timestamp_url = signing
    run([str(tool), "sign", "/s", "My", "/sha1", thumbprint, "/fd", "SHA256",
         "/tr", timestamp_url, "/td", "SHA256", str(path)], timeout=120)
    run([str(tool), "verify", "/pa", "/tw", "/v", str(path)], timeout=120)


def runtime_packages(metadata):
    tree = run(["cargo", "tree", "--locked", "-p", "nucelvo", "--target", TARGET,
                "--edges", "normal", "--prefix", "none"], capture_output=True, encoding="utf-8").stdout
    selected = {tuple(line.split()[:2]) for line in tree.splitlines() if line.strip()}
    return sorted(
        (package for package in metadata["packages"] if package["source"]
         and (package["name"], f"v{package['version']}") in selected),
        key=lambda package: (package["name"], package["version"]),
    )


def license_files(package):
    directory = Path(package["manifest_path"]).parent
    files = set()
    if package["license_file"]:
        explicit = (directory / package["license_file"]).resolve()
        if explicit.is_file() and explicit.is_relative_to(directory.resolve()):
            files.add(explicit)
    for path in directory.iterdir():
        name = path.name.lower()
        if path.is_file() and name.startswith(("license", "licence", "copying", "copyright", "notice", "ofl")):
            files.add(path)
        elif path.is_dir() and name in ("license", "licenses", "licences"):
            files.update(entry for entry in path.rglob("*") if entry.is_file())
    return sorted(files)


def write_notices(destination, makensis):
    metadata = json.loads(run(["cargo", "metadata", "--locked", "--format-version", "1",
                               "--filter-platform", TARGET], capture_output=True, encoding="utf-8").stdout)
    packages = runtime_packages(metadata)
    nsis_license = makensis.parent / "COPYING"
    if not nsis_license.is_file():
        raise SystemExit("The NSIS distribution must include its upstream COPYING file.")
    supplemental_groups = json.loads((SUPPLEMENTS / "packages.json").read_text(encoding="utf-8"))
    supplemental_paths = {
        package: sorted((SUPPLEMENTS / group).glob("LICENSE*"))
        for group, members in supplemental_groups.items()
        for package in members
    }
    with destination.open("wb") as output:
        def heading(text):
            output.write(("\n" + text + "\n").encode("utf-8"))

        heading("Nucelvo Windows distribution — third-party licenses")
        heading("Slint is used under the Slint Royalty-free License 2.0.")
        heading("Made with Slint — https://slint.dev/")
        heading("Public project and attribution — https://github.com/informigados/nucelvo")
        heading(f"Cargo dependency records for {TARGET} ({len(packages)} packages).")
        heading("Upstream notice and license files follow each package record.")
        heading("NSIS installer runtime — upstream COPYING")
        output.write(nsis_license.read_bytes())
        for package in packages:
            heading("=" * 72)
            heading(f"{package['name']} {package['version']} — {package['license'] or 'See upstream license file'}")
            heading(package["repository"] or f"https://crates.io/crates/{package['name']}/{package['version']}")
            key = f"{package['name']} {package['version']}"
            sources = license_files(package) or supplemental_paths.get(key)
            if not sources:
                raise SystemExit(f"No upstream license text was found for {key}.")
            for source in sources:
                heading(f"--- {source.name} ---")
                output.write(source.read_bytes())
                output.write(b"\n")
    return len(packages)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--makensis", help="Path to the NSIS 3 compiler")
    parser.add_argument("--signtool", help="Path to the Windows SDK SignTool executable")
    parser.add_argument("--signing-thumbprint", help="Code-signing certificate thumbprint in the current user's My store")
    parser.add_argument("--timestamp-url", help="HTTPS URL for the certificate provider's RFC 3161 timestamp service")
    args = parser.parse_args()
    if platform.system() != "Windows" or platform.machine().upper() not in {"AMD64", "X86_64"}:
        raise SystemExit("Build this package on native Windows x64.")
    makensis = find_makensis(args.makensis)
    signing = signing_options(args, parser)
    with (ROOT / "Cargo.toml").open("rb") as manifest:
        version = tomllib.load(manifest)["workspace"]["package"]["version"]
    version_parts = version.partition("-")[0].split(".")
    if len(version_parts) != 3 or not all(part.isdecimal() and 0 <= int(part) <= 65535 for part in version_parts):
        raise SystemExit("The Cargo version must have three Windows-compatible numeric components.")
    env = os.environ.copy()
    env.pop("RUSTFLAGS", None)
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env["CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS"] = "-C target-feature=+crt-static"
    target_dir = ROOT / "target"
    run(["cargo", "build", "--release", "--locked", "--target", TARGET, "--target-dir", str(target_dir)], env=env)
    binary = target_dir / TARGET / "release" / "nucelvo.exe"
    run([str(binary), "--smoke-test", "--require-logical-cpu"], timeout=45)

    stem = f"nucelvo-{version}-windows-x64"
    output = OUTPUT_ROOT / version
    OUTPUT_ROOT.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="nucelvo-package-", dir=OUTPUT_ROOT) as work:
        scratch = Path(work)
        payload = scratch / "payload"
        payload.mkdir()
        shutil.copy2(binary, payload / "nucelvo.exe")
        if signing:
            sign_file(payload / "nucelvo.exe", signing)
        for name in ("LICENSE", "THIRD_PARTY_NOTICES.md"):
            shutil.copy2(ROOT / name, payload / name)
        count = write_notices(payload / "THIRD_PARTY_LICENSES.txt", makensis)

        setup = scratch / f"{stem}-setup.exe"
        run([str(makensis), "/V3", "/INPUTCHARSET", "UTF8", f"/DNUCELVO_INSTALLER={setup}",
             f"/DNUCELVO_PAYLOAD={payload}", f"/DNUCELVO_ICON={ROOT / 'assets/brand/nucelvo-icon.ico'}",
             f"/DNUCELVO_VERSION={version}", f"/DNUCELVO_FILE_VERSION={'.'.join(version_parts)}.0",
             str(ROOT / "packaging/windows/Nucelvo.nsi")])
        if signing:
            sign_file(setup, signing)
        portable = scratch / f"{stem}-portable.zip"
        with zipfile.ZipFile(portable, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            for name in PAYLOAD:
                archive.write(payload / name, name)
        with zipfile.ZipFile(portable) as archive:
            if archive.testzip() is not None or set(archive.namelist()) != set(PAYLOAD):
                raise RuntimeError("Portable archive verification failed.")

        hashes = scratch / "SHA256SUMS"
        hashes.write_text("".join(
            f"{hashlib.sha256(file.read_bytes()).hexdigest()}  {file.name}\n"
            for file in (setup, portable)
        ), encoding="ascii", newline="\n")
        output.mkdir(exist_ok=True)
        for file in (setup, portable, hashes):
            file.replace(output / file.name)

    print(f"Built and verified Windows packages in {output}.")
    print(f"Included license records for {count} Cargo dependencies and NSIS.")


if __name__ == "__main__":
    main()
