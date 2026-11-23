use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr::{null, null_mut},
};

use nucelvo_core::{Field, State};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::{
        IO::DeviceIoControl,
        Ioctl::{
            DISK_EXTENT, IOCTL_STORAGE_PREDICT_FAILURE, IOCTL_STORAGE_QUERY_PROPERTY,
            NVMeDataTypeLogPage, ProtocolTypeNvme, StorageDeviceProtocolSpecificProperty,
        },
    },
};

use super::{Health, data_bytes, fields};

const NVME_SOURCE: &str = "Windows · NVMe health log";
const SMART_SOURCE: &str = "Windows · ATA SMART";
const HEADER: usize = 48;
const LOG_SIZE: usize = 512;

pub(in crate::details) fn collect(
    device: usize,
    index: u32,
    model: &str,
    interface: &str,
) -> Vec<Field> {
    let ata = interface.eq_ignore_ascii_case("IDE") || known_ata(model);
    let source = if ata { SMART_SOURCE } else { NVME_SOURCE };
    let reading = open_disk(index).and_then(|handle| {
        if ata {
            read_ata(&handle, model)
        } else {
            read_nvme(&handle)
        }
    });
    fields(device, source, reading)
}

fn known_ata(model: &str) -> bool {
    // Attribute 9 can have vendor-specific units. These two models were
    // checked on physical hardware with their ATA SMART attributes.
    matches!(model.trim(), "ST1000DX002-2DV162" | "TOSHIBA MK5065GSX")
}

fn open_disk(index: u32) -> Result<OwnedHandle, State> {
    open_device(&format!(r"\\.\PhysicalDrive{index}"))
}

fn open_device(device_path: &str) -> Result<OwnedHandle, State> {
    let name: Vec<u16> = device_path.encode_utf16().chain([0]).collect();
    // SAFETY: name is NUL-terminated and remains live for this call. The
    // handle requests zero read/write access and can only query properties.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(error_state());
    }
    // SAFETY: Windows returned a new owned handle, closed by OwnedHandle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub(in crate::details) fn volume_disk(volume: &str) -> Option<u32> {
    const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;
    #[repr(C)]
    struct Extents {
        count: u32,
        disks: [DISK_EXTENT; 64],
    }
    let [letter, b':'] = volume.as_bytes() else {
        return None;
    };
    if !letter.is_ascii_alphabetic() {
        return None;
    }
    let handle = open_device(&format!(r"\\.\{volume}")).ok()?;
    let mut result = Extents {
        count: 0,
        disks: [DISK_EXTENT::default(); 64],
    };
    let mut written = 0;
    // SAFETY: The volume was opened for read-only queries. This synchronous
    // call writes up to the length of the live, aligned extents buffer.
    let success = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            null(),
            0,
            (&raw mut result).cast(),
            std::mem::size_of::<Extents>() as u32,
            &mut written,
            null_mut(),
        )
    };
    let count = result.count as usize;
    if success == 0
        || count > result.disks.len()
        || (written as usize)
            < std::mem::offset_of!(Extents, disks) + count * std::mem::size_of::<DISK_EXTENT>()
    {
        return None;
    }
    single_disk(&result.disks[..count])
}

fn single_disk(extents: &[DISK_EXTENT]) -> Option<u32> {
    let disk = extents.first()?.DiskNumber;
    extents
        .iter()
        .all(|extent| extent.DiskNumber == disk)
        .then_some(disk)
}

fn error_state() -> State {
    match io::Error::last_os_error().raw_os_error() {
        Some(1 | 50 | 87) => State::Unsupported,
        Some(5) => State::PermissionDenied,
        Some(2 | 21 | 1167) => State::TemporarilyUnavailable,
        _ => State::Error,
    }
}

fn read_nvme(handle: &OwnedHandle) -> Result<Health, State> {
    let mut buffer = [0u8; HEADER + LOG_SIZE];
    // STORAGE_PROPERTY_QUERY followed by STORAGE_PROTOCOL_SPECIFIC_DATA.
    for (offset, value) in [
        (0, StorageDeviceProtocolSpecificProperty as u32),
        (8, ProtocolTypeNvme as u32),
        (12, NVMeDataTypeLogPage as u32),
        (16, 2),  // SMART / Health Information log page.
        (24, 40), // Offset from STORAGE_PROTOCOL_SPECIFIC_DATA.
        (28, LOG_SIZE as u32),
    ] {
        buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut received = 0;
    let pointer = buffer.as_mut_ptr().cast();
    // SAFETY: This buffered IOCTL uses the same live input and output buffer
    // and writes its completed byte count before the synchronous call returns.
    let success = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            IOCTL_STORAGE_QUERY_PROPERTY,
            pointer,
            buffer.len() as u32,
            pointer,
            buffer.len() as u32,
            &mut received,
            null_mut(),
        )
    };
    if success == 0 {
        return Err(error_state());
    }
    decode_nvme_response(buffer.get(..received as usize).ok_or(State::Error)?).ok_or(State::Error)
}

fn decode_nvme_response(buffer: &[u8]) -> Option<Health> {
    let word = |offset| {
        Some(u32::from_le_bytes(
            buffer.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    if word(0)? != HEADER as u32
        || word(4)? != HEADER as u32
        || word(8)? != ProtocolTypeNvme as u32
        || word(12)? != NVMeDataTypeLogPage as u32
        || word(24)? < 40
        || word(28)? < LOG_SIZE as u32
    {
        return None;
    }
    let start = (word(24)? as usize).checked_add(8)?;
    let log = buffer.get(start..start.checked_add(LOG_SIZE)?)?;
    if log.iter().all(|byte| *byte == 0) {
        return None;
    }
    let counter = |offset| {
        u64::try_from(u128::from_le_bytes(
            log[offset..offset + 16].try_into().unwrap(),
        ))
        .ok()
    };
    let kelvin = u16::from_le_bytes([log[1], log[2]]);
    Some(Health {
        passed: Some(log[0] == 0),
        temperature: kelvin
            .checked_sub(273)
            .filter(|value| *value <= 150)
            .map(u64::from),
        spare: (log[3] <= 100).then_some(u64::from(log[3])),
        wear: Some(u64::from(log[5])),
        hours: counter(128),
        cycles: counter(112),
        unsafe_shutdowns: counter(144),
        media_errors: counter(160),
        bytes_read: counter(32).and_then(data_bytes),
        bytes_written: counter(48).and_then(data_bytes),
        ..Health::default()
    })
}

fn read_ata(handle: &OwnedHandle, model: &str) -> Result<Health, State> {
    let mut buffer = [0u8; 4 + LOG_SIZE];
    let mut received = 0;
    // SAFETY: The synchronous IOCTL writes up to the length of the live
    // STORAGE_PREDICT_FAILURE output buffer. There is no input buffer.
    let success = unsafe {
        DeviceIoControl(
            handle.as_raw_handle(),
            IOCTL_STORAGE_PREDICT_FAILURE,
            null(),
            0,
            buffer.as_mut_ptr().cast(),
            buffer.len() as u32,
            &mut received,
            null_mut(),
        )
    };
    if success == 0 {
        return Err(error_state());
    }
    if received as usize != buffer.len() {
        return Err(State::Error);
    }
    Ok(decode_ata(&buffer, known_ata(model)))
}

fn decode_ata(buffer: &[u8; 4 + LOG_SIZE], known_model: bool) -> Health {
    let mut health = Health {
        passed: Some(buffer[..4] == [0; 4]),
        ..Health::default()
    };
    let page = &buffer[4..];
    if !known_model || page[..2] == [0; 2] || page.iter().copied().fold(0u8, u8::wrapping_add) != 0
    {
        return health;
    }
    let attribute = |id| {
        page[2..362]
            .as_chunks::<12>()
            .0
            .iter()
            .find(|record| record[0] == id)
            .map(|record| {
                let mut value = [0u8; 8];
                value[..6].copy_from_slice(&record[5..11]);
                u64::from_le_bytes(value)
            })
    };
    health.hours = attribute(9);
    health.cycles = attribute(12);
    health.temperature = attribute(194)
        .map(|value| value & 0xff)
        .filter(|value| *value <= 150);
    health.reallocated = attribute(5);
    health.pending = attribute(197);
    health.uncorrectable = attribute(198);
    health
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_associates_volume_extents_that_share_one_disk() {
        let first = DISK_EXTENT {
            DiskNumber: 2,
            ..Default::default()
        };
        let other = DISK_EXTENT {
            DiskNumber: 1,
            ..Default::default()
        };
        assert_eq!(single_disk(&[first, first]), Some(2));
        assert_eq!(single_disk(&[first, other]), None);
        assert_eq!(single_disk(&[]), None);
    }

    #[test]
    fn nvme_checks_layout_and_preserves_wear_above_rated_endurance() {
        let mut buffer = [0u8; HEADER + LOG_SIZE];
        for (offset, value) in [(0, 48u32), (4, 48), (8, 3), (12, 2), (24, 40), (28, 512)] {
            buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        buffer[HEADER] = 2;
        buffer[HEADER + 5] = 109;
        buffer[HEADER + 128..HEADER + 144].copy_from_slice(&4_455u128.to_le_bytes());
        let health = decode_nvme_response(&buffer).unwrap();
        assert_eq!(health.wear, Some(109));
        assert_eq!(health.hours, Some(4_455));
        assert_eq!(health.passed, Some(false));
        assert!(decode_nvme_response(&buffer[..HEADER + 20]).is_none());
        buffer[24..28].copy_from_slice(&12u32.to_le_bytes());
        assert!(decode_nvme_response(&buffer).is_none());
    }

    #[test]
    fn ata_hours_require_a_known_model_and_valid_checksum() {
        let mut buffer = [0u8; 4 + LOG_SIZE];
        buffer[4] = 10;
        buffer[6] = 9;
        buffer[11..13].copy_from_slice(&28_862u16.to_le_bytes());
        let sum = buffer[4..].iter().copied().fold(0u8, u8::wrapping_add);
        buffer[4 + LOG_SIZE - 1] = sum.wrapping_neg();
        assert_eq!(decode_ata(&buffer, true).hours, Some(28_862));
        assert_eq!(decode_ata(&buffer, false).hours, None);
        buffer[4] = 11;
        assert_eq!(decode_ata(&buffer, true).hours, None);
        assert_eq!(decode_ata(&buffer, true).passed, Some(true));
    }
}
