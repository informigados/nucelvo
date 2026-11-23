use futures::{
    executor::block_on,
    future::{Either, select},
};
use futures_timer::Delay;
use nucelvo_core::{Category as C, Field, Reading, Sample, State, Value};
use std::{collections::HashMap, io, mem::size_of, time::Duration};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_DESC1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIFactory1,
};
use windows_sys::Win32::System::SystemInformation::{
    GetTickCount64, GlobalMemoryStatusEx, MEMORYSTATUSEX,
};
use wmi::{Variant, WMIConnection, WMIError};

type Row = HashMap<String, Variant>;

struct Property {
    name: &'static str,
    key: &'static str,
    label: (&'static str, &'static str),
    unit: Option<&'static str>,
    private: bool,
}

const fn text(
    name: &'static str,
    key: &'static str,
    en: &'static str,
    pt: &'static str,
) -> Property {
    Property {
        name,
        key,
        label: (en, pt),
        unit: None,
        private: false,
    }
}

const fn number(
    name: &'static str,
    key: &'static str,
    en: &'static str,
    pt: &'static str,
    unit: &'static str,
) -> Property {
    Property {
        unit: Some(unit),
        ..text(name, key, en, pt)
    }
}

fn numeric(value: &Variant) -> Option<u64> {
    match value {
        Variant::UI1(v) => Some((*v).into()),
        Variant::UI2(v) => Some((*v).into()),
        Variant::UI4(v) => Some((*v).into()),
        Variant::UI8(v) => Some(*v),
        Variant::I4(v) => u64::try_from(*v).ok(),
        Variant::I8(v) => u64::try_from(*v).ok(),
        Variant::String(v) => v.parse().ok(),
        _ => None,
    }
}

fn error_state(error: &WMIError) -> State {
    match error {
        WMIError::HResultError {
            hres: -2147217405 | -2147024891,
        } => State::PermissionDenied,
        WMIError::HResultError {
            hres: -2147217394 | -2147217392,
        } => State::Unsupported,
        _ => State::Error,
    }
}

fn bounded_query(conn: &WMIConnection, query: &str) -> Result<Vec<Row>, State> {
    match block_on(select(
        Box::pin(conn.async_raw_query(query)),
        Delay::new(Duration::from_secs(3)),
    )) {
        Either::Left((result, _)) => result.map_err(|error| error_state(&error)),
        Either::Right(_) => Err(State::TemporarilyUnavailable),
    }
}

fn query(
    fields: &mut Vec<Field>,
    conn: &Result<WMIConnection, WMIError>,
    class: &'static str,
    category: C,
    first_device: usize,
    properties: &[Property],
) {
    let keys = properties
        .iter()
        .map(|field| field.name)
        .collect::<Vec<_>>()
        .join(", ");
    let filter = if class == "Win32_LogicalDisk" {
        " WHERE DriveType = 2 OR DriveType = 3 OR DriveType = 5"
    } else {
        ""
    };
    let disk_index = if class == "Win32_DiskDrive" {
        "Index, "
    } else {
        ""
    };
    let query = format!("SELECT {disk_index}{keys} FROM {class}{filter}");
    let (rows, missing) = match conn
        .as_ref()
        .map_err(error_state)
        .and_then(|conn| bounded_query(conn, &query))
    {
        Ok(rows) => (rows, State::Unsupported),
        Err(state) => (Vec::new(), state),
    };
    let make_field = |property: &Property, device, row: Option<&Row>| {
        let value = row.and_then(|row| row.get(property.name));
        let text = match value {
            Some(Variant::String(text)) if property.unit.is_none() => Some(text.clone()),
            _ => None,
        };
        let mut field = Field::text(category, device, property.key, property.label, text, class);
        if let Some(unit) = property.unit {
            let value = value.and_then(numeric).filter(|value| {
                !matches!(property.key, "configured_speed" | "memory_type") || *value != 0
            });
            field = match unit {
                "MHz" => field.number(value.and_then(|value| value.checked_mul(1_000_000)), "Hz"),
                "KiB" => field.number(value.and_then(|value| value.checked_mul(1024)), "bytes"),
                _ => field.number(value, unit),
            };
        }
        if row.is_none() {
            field.state = missing;
        }
        field.private = property.private;
        field
    };
    if rows.is_empty() {
        fields.extend(
            properties
                .iter()
                .map(|property| make_field(property, first_device, None)),
        );
    } else {
        for (index, row) in rows.iter().enumerate() {
            let device = first_device + index;
            fields.extend(
                properties
                    .iter()
                    .map(|property| make_field(property, device, Some(row))),
            );
            if class == "Win32_DiskDrive"
                && let Some(number) = row
                    .get("Index")
                    .and_then(numeric)
                    .and_then(|value| u32::try_from(value).ok())
            {
                let text = |key| match row.get(key) {
                    Some(Variant::String(text)) => text.as_str(),
                    _ => "",
                };
                fields.extend(super::storage_health::windows::collect(
                    device,
                    number,
                    text("Model"),
                    text("InterfaceType"),
                ));
            }
        }
    }
}

pub fn hardware_details() -> Vec<Field> {
    let conn = WMIConnection::new();
    let mut fields = Vec::new();
    query(
        &mut fields,
        &conn,
        "Win32_Processor",
        C::Cpu,
        0,
        &[
            text("Manufacturer", "manufacturer", "Manufacturer", "Fabricante"),
            number(
                "NumberOfCores",
                "physical_cores",
                "Physical cores",
                "Núcleos físicos",
                "",
            ),
            number(
                "MaxClockSpeed",
                "nominal_clock",
                "Reported maximum clock",
                "Clock máximo informado",
                "MHz",
            ),
            number("L2CacheSize", "l2_cache", "L2 cache", "Cache L2", "KiB"),
            number("L3CacheSize", "l3_cache", "L3 cache", "Cache L3", "KiB"),
        ],
    );
    query(
        &mut fields,
        &conn,
        "Win32_PhysicalMemory",
        C::Memory,
        1,
        &[
            text("DeviceLocator", "slot", "Memory slot", "Slot de memória"),
            text(
                "Manufacturer",
                "manufacturer",
                "Module manufacturer",
                "Fabricante do módulo",
            ),
            number(
                "Capacity",
                "capacity",
                "Module capacity",
                "Capacidade do módulo",
                "bytes",
            ),
            number(
                "ConfiguredClockSpeed",
                "configured_speed",
                "Configured speed",
                "Velocidade configurada",
                "MHz",
            ),
            number(
                "SMBIOSMemoryType",
                "memory_type",
                "SMBIOS memory type",
                "Tipo de memória SMBIOS",
                "",
            ),
        ],
    );
    query(
        &mut fields,
        &conn,
        "Win32_BaseBoard",
        C::Board,
        0,
        &[
            text("Manufacturer", "manufacturer", "Manufacturer", "Fabricante"),
            text("Product", "model", "Board model", "Modelo da placa"),
            Property {
                private: true,
                ..text(
                    "SerialNumber",
                    "serial",
                    "Board serial number",
                    "Número de série da placa",
                )
            },
        ],
    );
    query(
        &mut fields,
        &conn,
        "Win32_BIOS",
        C::Board,
        0,
        &[text(
            "SMBIOSBIOSVersion",
            "firmware_version",
            "Firmware version",
            "Versão do firmware",
        )],
    );
    query(
        &mut fields,
        &conn,
        "Win32_VideoController",
        C::Graphics,
        0,
        &[
            text("Name", "model", "Adapter", "Adaptador"),
            text(
                "DriverVersion",
                "driver",
                "Driver version",
                "Versão do driver",
            ),
        ],
    );
    query(
        &mut fields,
        &conn,
        "Win32_DiskDrive",
        C::Storage,
        0,
        &[
            text("Model", "model", "Disk model", "Modelo do disco"),
            text(
                "InterfaceType",
                "interface",
                "Reported interface",
                "Interface informada",
            ),
            number(
                "Size",
                "capacity",
                "Disk capacity",
                "Capacidade do disco",
                "bytes",
            ),
            Property {
                private: true,
                ..text(
                    "SerialNumber",
                    "serial",
                    "Disk serial number",
                    "Número de série do disco",
                )
            },
        ],
    );
    query(
        &mut fields,
        &conn,
        "Win32_LogicalDisk",
        C::Storage,
        10_000,
        &[
            Property {
                private: true,
                ..text(
                    "DeviceID",
                    "mount_point",
                    "Volume path",
                    "Caminho do volume",
                )
            },
            Property {
                private: true,
                ..text("VolumeName", "volume_name", "Volume name", "Nome do volume")
            },
            text(
                "FileSystem",
                "filesystem",
                "File system",
                "Sistema de arquivos",
            ),
            number(
                "Size",
                "volume_capacity",
                "Volume capacity",
                "Capacidade do volume",
                "bytes",
            ),
            number(
                "FreeSpace",
                "free_space",
                "Free space",
                "Espaço livre",
                "bytes",
            ),
        ],
    );
    // SAFETY: GetTickCount64 takes no arguments and reads the system uptime.
    fields.push(
        Field::text(
            C::System,
            0,
            "uptime",
            ("Uptime at capture", "Tempo ligado na captura"),
            None,
            "Win32 · GetTickCount64",
        )
        .number(Some(unsafe { GetTickCount64() } / 1000), "s"),
    );
    graphics_memory(&mut fields);
    fields
}

fn graphics_memory(fields: &mut Vec<Field>) {
    const SOURCE: &str = "DXGI · IDXGIAdapter1";
    let mut adapters = Vec::new();
    // SAFETY: the typed factory request uses the corresponding Windows interface identifier.
    if let Ok(factory) = unsafe { CreateDXGIFactory1::<IDXGIFactory1>() } {
        for index in 0..64 {
            // SAFETY: the factory owns a valid DXGI handle and the index is checked by DXGI.
            let Ok(adapter) = (unsafe { factory.EnumAdapters1(index) }) else {
                break;
            };
            // SAFETY: DXGI fills a correctly typed descriptor for the valid adapter handle.
            let Ok(desc) = (unsafe { adapter.GetDesc1() }) else {
                continue;
            };
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }
            let end = desc
                .Description
                .iter()
                .position(|char| *char == 0)
                .unwrap_or(desc.Description.len());
            adapters.push((String::from_utf16_lossy(&desc.Description[..end]), desc));
        }
    }
    let existing = fields
        .iter()
        .filter_map(|field| match (&field.value, field.category, field.key) {
            (Some(Value::Text(name)), C::Graphics, "model") => Some((field.device, name.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut used = Vec::new();
    for (index, (name, desc)) in adapters.iter().enumerate() {
        let mut matches = existing
            .iter()
            .filter(|(_, label)| label.eq_ignore_ascii_case(name));
        let device = match (matches.next(), matches.next()) {
            (Some((device, _)), None)
                if adapters
                    .iter()
                    .filter(|(label, _)| label.eq_ignore_ascii_case(name))
                    .count()
                    == 1 =>
            {
                *device
            }
            _ => {
                let next = existing.len() + index + usize::from(existing.is_empty());
                fields.push(Field::text(
                    C::Graphics,
                    next,
                    "model",
                    ("Adapter", "Adaptador"),
                    Some(name.clone()),
                    SOURCE,
                ));
                next
            }
        };
        used.push(device);
        add_graphics_memory(fields, device, Some(desc), SOURCE);
    }
    for (device, _) in existing {
        if !used.contains(&device) {
            add_graphics_memory(fields, device, None, SOURCE);
        }
    }
}

fn add_graphics_memory(
    fields: &mut Vec<Field>,
    device: usize,
    desc: Option<&DXGI_ADAPTER_DESC1>,
    source: &'static str,
) {
    for (key, labels, bytes) in [
        (
            "dedicated_vram",
            ("Dedicated VRAM", "VRAM dedicada"),
            desc.map(|desc| desc.DedicatedVideoMemory),
        ),
        (
            "shared_memory",
            ("Shared system memory", "Memória de sistema compartilhada"),
            desc.map(|desc| desc.SharedSystemMemory),
        ),
    ] {
        fields.push(
            Field::text(C::Graphics, device, key, labels, None, source)
                .number(bytes.map(|bytes| bytes as u64), "bytes"),
        );
    }
}

pub fn memory_load() -> io::Result<f64> {
    let mut status = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: status is a writable structure with its size initialized as required by Win32.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(f64::from(status.dwMemoryLoad))
}

pub fn sensors() -> Vec<Sample> {
    const SOURCE: &str = "WMI · MSAcpi_ThermalZoneTemperature";
    let rows = match WMIConnection::with_namespace_path("ROOT\\WMI")
        .map_err(|error| error_state(&error))
        .and_then(|conn| {
            bounded_query(
                &conn,
                "SELECT InstanceName, CurrentTemperature FROM MSAcpi_ThermalZoneTemperature",
            )
        }) {
        Ok(rows) if !rows.is_empty() => rows,
        Ok(_) => return super::unavailable_sensor(SOURCE, State::Unsupported),
        Err(state) => return super::unavailable_sensor(SOURCE, state),
    };
    rows.into_iter()
        .enumerate()
        .map(|(index, row)| {
            let id = match row.get("InstanceName") {
                Some(Variant::String(name)) => format!("acpi:{name}"),
                _ => format!("acpi:{index}"),
            };
            let value = row
                .get("CurrentTemperature")
                .and_then(numeric)
                .filter(|value| *value > 0)
                .map(|v| v as f64 / 10.0 - 273.15);
            super::sample(
                id,
                format!("ACPI #{}", index + 1),
                None,
                "°C",
                Reading {
                    source: SOURCE,
                    state: if value.is_some() {
                        State::Ok
                    } else {
                        State::Unsupported
                    },
                    value,
                },
            )
        })
        .collect()
}
