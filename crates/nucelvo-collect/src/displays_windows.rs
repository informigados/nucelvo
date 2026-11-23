use std::{mem::size_of, ptr::null};

use nucelvo_core::{Category, Field};
use windows_sys::Win32::Graphics::Gdi::{
    DEVMODEW, DISPLAY_DEVICE_ATTACHED_TO_DESKTOP, DISPLAY_DEVICE_MIRRORING_DRIVER,
    DISPLAY_DEVICE_PRIMARY_DEVICE, DISPLAY_DEVICEW, ENUM_CURRENT_SETTINGS, EnumDisplayDevicesW,
    EnumDisplaySettingsW,
};

const SOURCE: &str = "Win32 · Display settings";
const FIRST_DISPLAY: usize = 10_000;

pub(super) fn fields() -> Vec<Field> {
    let mut fields = Vec::new();
    for index in 0..64 {
        let Some(display) = device(null(), index) else {
            break;
        };
        if display.StateFlags & DISPLAY_DEVICE_ATTACHED_TO_DESKTOP == 0
            || display.StateFlags & DISPLAY_DEVICE_MIRRORING_DRIVER != 0
        {
            continue;
        }
        let group = FIRST_DISPLAY + fields.len() / 4;
        let model = device(display.DeviceName.as_ptr(), 0).map(|monitor| {
            let end = monitor
                .DeviceString
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(monitor.DeviceString.len());
            String::from_utf16_lossy(&monitor.DeviceString[..end])
        });
        let mut mode = DEVMODEW {
            dmSize: size_of::<DEVMODEW>() as u16,
            ..Default::default()
        };
        // SAFETY: DeviceName is a NUL-terminated name returned by EnumDisplayDevicesW.
        // mode points to a writable DEVMODEW of the declared size with no driver extra.
        let current = unsafe {
            EnumDisplaySettingsW(
                display.DeviceName.as_ptr(),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
        } != 0;
        let text =
            |key, labels, value| Field::text(Category::System, group, key, labels, value, SOURCE);
        fields.extend([
            text(
                "display_name",
                ("Display model", "Modelo do monitor"),
                model,
            ),
            text(
                "display_resolution",
                ("Current resolution", "Resolução atual"),
                (current && mode.dmPelsWidth > 0 && mode.dmPelsHeight > 0)
                    .then(|| format!("{} × {}", mode.dmPelsWidth, mode.dmPelsHeight)),
            ),
            text(
                "display_refresh",
                ("Refresh rate", "Taxa de atualização"),
                None,
            )
            .number(
                (current && mode.dmDisplayFrequency > 1)
                    .then_some(u64::from(mode.dmDisplayFrequency)),
                "Hz",
            ),
            text(
                "display_role",
                ("Desktop role", "Função na área de trabalho"),
                Some(
                    if display.StateFlags & DISPLAY_DEVICE_PRIMARY_DEVICE != 0 {
                        "primary"
                    } else {
                        "secondary"
                    }
                    .into(),
                ),
            ),
        ]);
    }
    fields
}

fn device(parent: *const u16, index: u32) -> Option<DISPLAY_DEVICEW> {
    let mut display = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // SAFETY: parent is either null or a live device name obtained from this API.
    // display is writable with cb set to the corresponding structure size.
    let found = unsafe { EnumDisplayDevicesW(parent, index, &mut display, 0) };
    (found != 0).then_some(display)
}
