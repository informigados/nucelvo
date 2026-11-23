use std::{io, mem::size_of};
use windows_sys::Win32::{
    Foundation::FILETIME,
    System::{
        SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
        Threading::{GetActiveProcessorGroupCount, GetSystemTimes},
    },
};
use winreg::{RegKey, enums::HKEY_LOCAL_MACHINE};

use super::CpuTicks;

pub const CPU_SOURCE: &str = "Windows Registry · CentralProcessor";
pub const MEMORY_SOURCE: &str = "Win32 · GlobalMemoryStatusEx";
pub const OS_SOURCE: &str = "Windows Registry · CurrentVersion";
pub const USAGE_SOURCE: &str = "Win32 · GetSystemTimes";

pub fn processor() -> io::Result<String> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0")?;
    let name: String = key.get_value("ProcessorNameString")?;
    if name.trim().is_empty() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(name.trim().to_owned())
}

pub fn memory_bytes() -> io::Result<u64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is an initialized writable structure with its length set as Win32 requires.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.ullTotalPhys)
}

pub fn operating_system() -> io::Result<String> {
    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")?;
    let build: String = key.get_value("CurrentBuildNumber")?;
    let build = build
        .parse::<u32>()
        .map_err(|_| io::ErrorKind::InvalidData)?;
    let revision: u32 = key.get_value("UBR")?;
    let version: String = key
        .get_value("DisplayVersion")
        .or_else(|_| key.get_value("ReleaseId"))?;
    let installation: String = key.get_value("InstallationType")?;
    let name = if installation == "Client" && build >= 22000 {
        String::from("Windows 11")
    } else {
        key.get_value("ProductName")?
    };
    Ok(format!("{name} {version} · {build}.{revision}"))
}

pub fn cpu_ticks() -> io::Result<CpuTicks> {
    // SAFETY: this Win32 function takes no arguments and reads system topology.
    if unsafe { GetActiveProcessorGroupCount() } > 1 {
        return Err(io::ErrorKind::Unsupported.into());
    }
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    // SAFETY: each pointer refers to a different, writable FILETIME for the duration of the call.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let ticks =
        |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    let idle = ticks(idle);
    let busy = ticks(kernel)
        .checked_sub(idle)
        .and_then(|kernel_busy| kernel_busy.checked_add(ticks(user)))
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok(CpuTicks { busy, idle })
}
