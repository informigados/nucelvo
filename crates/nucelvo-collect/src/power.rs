use std::io;

use nucelvo_core::{Category, Field, Reading};

#[derive(Debug, Default, PartialEq, Eq)]
struct Power {
    source: Option<&'static str>,
    battery_detected: Option<bool>,
    batteries: Vec<Battery>,
}

#[derive(Debug, PartialEq, Eq)]
struct Battery {
    status: Option<&'static str>,
    charge: Option<u64>,
}

pub(super) fn fields() -> Vec<Field> {
    #[cfg(windows)]
    let (result, source) = (windows::read(), "Win32 · GetSystemPowerStatus");
    #[cfg(target_os = "linux")]
    let (result, source) = (
        linux::read(std::path::Path::new("/sys/class/power_supply")),
        "sysfs · power_supply",
    );
    #[cfg(target_os = "macos")]
    let (result, source) = (
        Err::<Power, _>(io::Error::from(io::ErrorKind::Unsupported)),
        "macOS",
    );
    let reading = Reading::from_result(result, source);
    let power = reading.value.unwrap_or_default();
    let text = |device, key, labels, value: Option<&str>| {
        let mut field = Field::text(
            Category::System,
            device,
            key,
            labels,
            value.map(str::to_owned),
            source,
        );
        if reading.state != nucelvo_core::State::Ok {
            field.state = reading.state;
        }
        field
    };
    let mut fields = vec![
        text(
            1,
            "power_source",
            ("Power source", "Fonte de energia"),
            power.source,
        ),
        text(
            1,
            "battery_detected",
            ("System battery", "Bateria do sistema"),
            power
                .battery_detected
                .map(|detected| if detected { "detected" } else { "not_detected" }),
        ),
    ];
    let multiple = power.batteries.len() > 1;
    for (index, battery) in power.batteries.into_iter().enumerate() {
        let device = if multiple { index + 2 } else { 1 };
        fields.push(text(
            device,
            "battery_status",
            ("Battery status", "Estado da bateria"),
            battery.status,
        ));
        fields.push(
            text(
                device,
                "battery_charge",
                ("Battery charge", "Carga da bateria"),
                None,
            )
            .number(battery.charge, "%"),
        );
    }
    fields
}

#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    pub(super) fn read() -> io::Result<Power> {
        let mut status = SYSTEM_POWER_STATUS::default();
        // SAFETY: The API fills the writable structure provided by the caller.
        if unsafe { GetSystemPowerStatus(&mut status) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(decode(
            status.ACLineStatus,
            status.BatteryFlag,
            status.BatteryLifePercent,
        ))
    }

    fn decode(ac: u8, flags: u8, percent: u8) -> Power {
        let charge = (percent <= 100).then_some(u64::from(percent));
        let battery_detected = match flags {
            255 => charge.map(|_| true),
            flags => Some(flags & 128 == 0),
        };
        let source = match (ac, battery_detected) {
            (1, _) => Some("external"),
            (0, Some(true)) => Some("battery"),
            _ => None,
        };
        let status = match (flags, source) {
            (255, _) => None,
            (flag, _) if flag & 8 != 0 => Some("charging"),
            (_, Some("battery")) => Some("discharging"),
            _ => Some("not_charging"),
        };
        Power {
            source,
            battery_detected,
            batteries: if battery_detected == Some(true) {
                vec![Battery { status, charge }]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn distinguishes_missing_battery_unknown_fields_and_zero_charge() {
        let desktop = decode(1, 128, 255);
        assert_eq!(desktop.source, Some("external"));
        assert_eq!(desktop.battery_detected, Some(false));
        assert!(desktop.batteries.is_empty());
        assert_eq!(decode(255, 255, 255), Power::default());
        let empty = decode(0, 4, 0);
        assert_eq!(
            empty.batteries[0],
            Battery {
                status: Some("discharging"),
                charge: Some(0)
            }
        );
        assert_eq!(decode(1, 8, 45).batteries[0].status, Some("charging"));
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use super::*;
    use std::{fs, path::Path};

    fn attribute(path: &Path, name: &str) -> io::Result<String> {
        fs::read_to_string(path.join(name)).map(|value| value.trim().to_owned())
    }

    pub(super) fn read(root: &Path) -> io::Result<Power> {
        let mut paths = fs::read_dir(root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<_>>>()?;
        paths.sort_unstable();
        let mut batteries = Vec::new();
        let mut complete = true;
        let mut detected_online = false;
        let mut detected_offline = false;
        let mut source_unknown = false;
        for path in paths {
            if attribute(&path, "scope")
                .as_deref()
                .is_ok_and(|scope| scope == "Device")
            {
                continue;
            }
            let Ok(kind) = attribute(&path, "type") else {
                complete = false;
                continue;
            };
            match kind.as_str() {
                "Battery" => {
                    match attribute(&path, "present") {
                        Ok(value) if value == "0" => continue,
                        Ok(value) if value == "1" => {}
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        _ => {
                            complete = false;
                            continue;
                        }
                    }
                    let status =
                        attribute(&path, "status")
                            .ok()
                            .and_then(|value| match value.as_str() {
                                "Charging" => Some("charging"),
                                "Discharging" => Some("discharging"),
                                "Not charging" => Some("not_charging"),
                                "Full" => Some("full"),
                                _ => None,
                            });
                    let charge = attribute(&path, "capacity")
                        .ok()
                        .and_then(|value| value.parse::<u64>().ok())
                        .filter(|value| *value <= 100);
                    batteries.push(Battery { status, charge });
                }
                "Mains" | "USB" | "USB_DCP" | "USB_CDP" | "USB_ACA" | "USB_C" | "USB_PD"
                | "USB_PD_DRP" | "Wireless" | "BrickID" => {
                    match attribute(&path, "online").as_deref() {
                        Ok("1") => detected_online = true,
                        Ok("0") => detected_offline = true,
                        _ => source_unknown = true,
                    }
                }
                _ => {}
            }
        }
        let source = if detected_online {
            Some("external")
        } else if complete && detected_offline && !source_unknown && !batteries.is_empty() {
            Some("battery")
        } else {
            None
        };
        let battery_detected = if batteries.is_empty() {
            complete.then_some(false)
        } else {
            Some(true)
        };
        Ok(Power {
            source,
            battery_detected,
            batteries,
        })
    }

    #[test]
    fn reads_each_system_pack_and_ignores_peripherals_and_empty_slots() {
        let root = tempfile::tempdir().unwrap();
        let supply = |name, attributes: &[(&str, &str)]| {
            let path = root.path().join(name);
            fs::create_dir(&path).unwrap();
            for (attribute, value) in attributes {
                fs::write(path.join(attribute), value).unwrap();
            }
        };
        supply("AC", &[("type", "Mains\n"), ("online", "1\n")]);
        supply(
            "BAT0",
            &[
                ("type", "Battery"),
                ("status", "Charging"),
                ("capacity", "52"),
            ],
        );
        supply(
            "BAT1",
            &[
                ("type", "Battery"),
                ("present", "1"),
                ("status", "Unknown"),
                ("capacity", "255"),
            ],
        );
        supply("BAT2", &[("type", "Battery"), ("present", "0")]);
        supply(
            "mouse",
            &[("type", "Battery"), ("scope", "Device"), ("capacity", "75")],
        );
        let power = read(root.path()).unwrap();
        assert_eq!(power.source, Some("external"));
        assert_eq!(power.battery_detected, Some(true));
        assert_eq!(
            power.batteries,
            [
                Battery {
                    status: Some("charging"),
                    charge: Some(52)
                },
                Battery {
                    status: None,
                    charge: None
                },
            ]
        );
        fs::write(root.path().join("AC/online"), "0").unwrap();
        assert_eq!(read(root.path()).unwrap().source, Some("battery"));
    }
}
