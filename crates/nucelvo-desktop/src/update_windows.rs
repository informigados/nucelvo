use std::{
    fs::{self, File},
    io::{self, Read, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use ring::digest::{Context, SHA256};
use semver::Version;
use serde::Deserialize;
use slint::ComponentHandle;
use tempfile::TempDir;
use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::Shell::{AttachmentServices, IAttachmentExecute},
    },
    core::{PCWSTR, w},
};
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY},
};

use super::{MainWindow, REPOSITORY};

const MAX_INSTALLER_SIZE: u64 = 128 * 1024 * 1024;

#[derive(Deserialize)]
pub(super) struct Asset {
    name: String,
    state: String,
    size: u64,
    digest: Option<String>,
}

#[derive(Clone)]
pub(super) struct Installer {
    name: String,
    url: String,
    size: u64,
    digest: [u8; 32],
}

impl Installer {
    pub(super) fn from_assets(version: &Version, assets: &[Asset]) -> Option<Self> {
        let name = format!("nucelvo-{version}-windows-x64-setup.exe");
        let mut matches = assets.iter().filter(|asset| asset.name == name);
        let asset = matches.next()?;
        if matches.next().is_some()
            || asset.state != "uploaded"
            || !(1..=MAX_INSTALLER_SIZE).contains(&asset.size)
        {
            return None;
        }
        let hex = asset.digest.as_deref()?.strip_prefix("sha256:")?;
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut digest = [0; 32];
        for (index, output) in digest.iter_mut().enumerate() {
            *output = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Self {
            url: format!("{REPOSITORY}/releases/download/v{version}/{name}"),
            name,
            size: asset.size,
            digest,
        })
    }

    fn verified_copy(&self, input: impl Read, mut output: impl Write) -> io::Result<()> {
        let mut hash = Context::new(&SHA256);
        let mut input = input.take(self.size + 1);
        let mut total = 0;
        let mut buffer = [0; 32 * 1024];
        loop {
            let length = input.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            total += length as u64;
            if total > self.size {
                return Err(io::ErrorKind::InvalidData.into());
            }
            hash.update(&buffer[..length]);
            output.write_all(&buffer[..length])?;
        }
        if total != self.size || hash.finish().as_ref() != self.digest {
            return Err(io::ErrorKind::InvalidData.into());
        }
        Ok(())
    }

    fn download(self) -> Result<StagedUpdate, ()> {
        let client = reqwest::blocking::Client::builder()
            .https_only(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(90))
            .user_agent(concat!("Nucelvo/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                let url = attempt.url();
                if attempt.previous().len() < 4
                    && url.scheme() == "https"
                    && url.port_or_known_default() == Some(443)
                    && url.username().is_empty()
                    && url.password().is_none()
                    && matches!(
                        url.host_str(),
                        Some(
                            "github.com"
                                | "release-assets.githubusercontent.com"
                                | "objects.githubusercontent.com"
                        )
                    )
                {
                    attempt.follow()
                } else {
                    attempt.error("Update redirect is outside GitHub")
                }
            }))
            .build()
            .map_err(|_| ())?;
        let response = client
            .get(&self.url)
            .send()
            .and_then(|response| response.error_for_status())
            .map_err(|_| ())?;
        if response.status() != reqwest::StatusCode::OK
            || response
                .content_length()
                .is_some_and(|length| length != self.size)
        {
            return Err(());
        }
        let directory = tempfile::Builder::new()
            .prefix("nucelvo-update-")
            .tempdir()
            .map_err(|_| ())?;
        let path = directory.path().join(&self.name);
        {
            let mut output = File::create_new(&path).map_err(|_| ())?;
            self.verified_copy(response, &mut output).map_err(|_| ())?;
            output.sync_all().map_err(|_| ())?;
        }
        with_attachment(&path, &self.url, |attachment| {
            // SAFETY: The attachment has a local path and HTTPS source, and COM is initialized.
            unsafe { attachment.Save() }
        })
        .map_err(|_| ())?;
        self.verified_copy(File::open(&path).map_err(|_| ())?, io::sink())
            .map_err(|_| ())?;
        Ok(StagedUpdate {
            installer: self,
            directory,
            path,
        })
    }
}

struct StagedUpdate {
    installer: Installer,
    directory: TempDir,
    path: PathBuf,
}

impl StagedUpdate {
    fn launch(self) -> Result<(), ()> {
        // Recheck the staged bytes immediately before passing the file to Windows.
        self.installer
            .verified_copy(File::open(&self.path).map_err(|_| ())?, io::sink())
            .map_err(|_| ())?;
        with_attachment(&self.path, &self.installer.url, |attachment| {
            let mut process = HANDLE::default();
            // SAFETY: Windows performs its attachment policy checks and returns an owned process handle.
            unsafe {
                attachment.Execute(HWND::default(), w!("open"), Some(&mut process))?;
                if !process.is_invalid() {
                    let _ = CloseHandle(process);
                }
            }
            Ok(())
        })
        .map_err(|_| ())?;
        // Setup is interactive and may still be using its executable after Nucelvo exits.
        let _ = self.directory.keep();
        Ok(())
    }
}

fn with_attachment(
    path: &Path,
    source: &str,
    action: impl FnOnce(&IAttachmentExecute) -> windows::core::Result<()>,
) -> windows::core::Result<()> {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: The current worker thread initialized COM successfully.
            unsafe { CoUninitialize() };
        }
    }

    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let source: Vec<u16> = source.encode_utf16().chain([0]).collect();
    // SAFETY: This runs on a dedicated worker. Both UTF-16 strings are terminated and live for all calls.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
        let _apartment = Apartment;
        let attachment: IAttachmentExecute =
            CoCreateInstance(&AttachmentServices, None, CLSCTX_INPROC_SERVER)?;
        attachment.SetLocalPath(PCWSTR(path.as_ptr()))?;
        attachment.SetSource(PCWSTR(source.as_ptr()))?;
        action(&attachment)
    }
}

pub(super) fn is_installed() -> bool {
    let location = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Nucelvo",
            KEY_READ | KEY_WOW64_64KEY,
        )
        .and_then(|key| key.get_value::<String, _>("InstallLocation"))
        .ok()
        .filter(|path| Path::new(path).is_absolute());
    let expected =
        location.and_then(|path| fs::canonicalize(Path::new(&path).join("nucelvo.exe")).ok());
    expected.is_some() && std::env::current_exe().and_then(fs::canonicalize).ok() == expected
}

#[derive(Default)]
pub(super) struct Update {
    installer: Option<Installer>,
    staged: Option<StagedUpdate>,
}

impl Update {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn offer(&mut self, installer: Installer) {
        self.installer = Some(installer);
    }
}

pub(super) fn bind(window: &MainWindow) -> Arc<Mutex<Update>> {
    let state = Arc::new(Mutex::new(Update::default()));
    let weak = window.as_weak();
    let update = state.clone();
    window.on_download_update(move || {
        let Some(window) = weak.upgrade() else { return };
        if !window.get_update_installable() || !matches!(window.get_update_download_state(), 0 | 4)
        {
            return;
        }
        let Some(installer) = update.lock().unwrap().installer.clone() else {
            return;
        };
        window.set_update_download_state(1);
        let weak = window.as_weak();
        let update = update.clone();
        if thread::Builder::new()
            .name("download-update".into())
            .spawn(move || {
                let result = installer.download();
                let _ = weak.upgrade_in_event_loop(move |window| match result {
                    Ok(staged) => {
                        update.lock().unwrap().staged = Some(staged);
                        window.set_update_download_state(2);
                    }
                    Err(()) => window.set_update_download_state(4),
                });
            })
            .is_err()
        {
            window.set_update_download_state(4);
        }
    });

    let weak = window.as_weak();
    let update = state.clone();
    window.on_install_update(move || {
        let Some(window) = weak.upgrade() else { return };
        if window.get_update_download_state() != 2 || window.get_export_busy() || !is_installed() {
            return;
        }
        let Some(staged) = update.lock().unwrap().staged.take() else {
            return;
        };
        window.set_update_download_state(3);
        let weak = window.as_weak();
        if thread::Builder::new()
            .name("install-update".into())
            .spawn(move || {
                let result = staged.launch();
                let _ = weak.upgrade_in_event_loop(move |window| {
                    if result.is_ok() {
                        let _ = slint::quit_event_loop();
                    } else {
                        window.set_update_download_state(4);
                    }
                });
            })
            .is_err()
        {
            window.set_update_download_state(4);
        }
    });
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_and_verifies_only_the_expected_release_installer() {
        let version = Version::parse("0.7.0").unwrap();
        let mut asset = Asset {
            name: "nucelvo-0.7.0-windows-x64-setup.exe".into(),
            state: "uploaded".into(),
            size: 3,
            digest: Some(
                "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            ),
        };
        let installer = Installer::from_assets(&version, std::slice::from_ref(&asset)).unwrap();
        assert!(installer.verified_copy(&b"abc"[..], io::sink()).is_ok());
        for invalid in [&b"ab"[..], &b"abcd"[..], &b"abd"[..]] {
            assert!(installer.verified_copy(invalid, io::sink()).is_err());
        }
        asset.digest = None;
        assert!(Installer::from_assets(&version, &[asset]).is_none());
    }

    #[test]
    #[ignore = "uses the public GitHub release and Windows Attachment Manager"]
    fn downloads_and_stages_official_installer() {
        let asset = Asset {
            name: "nucelvo-0.7.0-windows-x64-setup.exe".into(),
            state: "uploaded".into(),
            size: 6_885_788,
            digest: Some(
                "sha256:72fd5a97790bc97e7e98de2cf0311f5ba61ee7c9c4efa475e1f2c4ef6522716f".into(),
            ),
        };
        let staged = Installer::from_assets(&Version::new(0, 7, 0), &[asset])
            .unwrap()
            .download()
            .unwrap();
        assert_eq!(staged.path.metadata().unwrap().len(), 6_885_788);
    }
}
