use nucelvo_core::{Category, Field};

#[derive(Default)]
struct Adapter {
    name: String,
    kind: &'static str,
    state: Option<&'static str>,
    rx_speed: Option<u64>,
    tx_speed: Option<u64>,
    received: Option<u64>,
    sent: Option<u64>,
}

pub(super) fn fields() -> Vec<Field> {
    #[cfg(windows)]
    let (adapters, source) = (windows::read(), "Win32 · IP Helper");
    #[cfg(target_os = "linux")]
    let (adapters, source) = (
        linux::read(std::path::Path::new("/sys/class/net")),
        "sysfs · network",
    );
    let mut fields = Vec::new();
    for (index, adapter) in adapters.into_iter().enumerate() {
        let text = |key, labels, value| {
            Field::text(Category::System, 20_000 + index, key, labels, value, source)
        };
        let mut name = text("network_name", ("Adapter", "Adaptador"), Some(adapter.name));
        // Interface names can be customized on Linux. Keep them out of share-safe reports.
        name.private = cfg!(target_os = "linux");
        fields.extend([
            name,
            text(
                "network_type",
                ("Connection type", "Tipo de conexão"),
                Some(adapter.kind.into()),
            ),
            text(
                "network_status",
                ("Link status", "Estado da conexão"),
                adapter.state.map(str::to_owned),
            ),
        ]);
        if adapter.rx_speed == adapter.tx_speed {
            fields.push(
                text(
                    "network_link_speed",
                    ("Link speed", "Velocidade do link"),
                    None,
                )
                .number(adapter.rx_speed, "bit/s"),
            );
        } else {
            fields.extend([
                text(
                    "network_rx_speed",
                    ("Receive link speed", "Velocidade do link de recepção"),
                    None,
                )
                .number(adapter.rx_speed, "bit/s"),
                text(
                    "network_tx_speed",
                    ("Transmit link speed", "Velocidade do link de transmissão"),
                    None,
                )
                .number(adapter.tx_speed, "bit/s"),
            ]);
        }
        fields.extend([
            text(
                "network_received",
                (
                    "Data received by the adapter",
                    "Dados recebidos pelo adaptador",
                ),
                None,
            )
            .number(adapter.received, "bytes"),
            text(
                "network_sent",
                ("Data sent by the adapter", "Dados enviados pelo adaptador"),
                None,
            )
            .number(adapter.sent, "bytes"),
        ]);
    }
    if fields.is_empty() {
        fields.push(Field::text(
            Category::System,
            20_000,
            "network_status",
            ("Physical network adapters", "Adaptadores de rede físicos"),
            None,
            source,
        ));
    }
    fields
}

#[cfg(windows)]
mod windows {
    use super::Adapter;
    use std::ptr::{addr_of, null_mut};
    use windows_sys::Win32::NetworkManagement::{
        IpHelper::{
            FreeMibTable, GetIfTable2, IF_TYPE_ETHERNET_CSMACD, IF_TYPE_IEEE80211, MIB_IF_ROW2,
            MIB_IF_TABLE2,
        },
        Ndis::{
            IfOperStatusDormant, IfOperStatusDown, IfOperStatusLowerLayerDown, IfOperStatusTesting,
            IfOperStatusUp,
        },
    };

    struct Table(*mut MIB_IF_TABLE2);

    impl Drop for Table {
        fn drop(&mut self) {
            // SAFETY: GetIfTable2 allocated this non-null pointer.
            unsafe { FreeMibTable(self.0.cast()) };
        }
    }

    pub(super) fn read() -> Vec<Adapter> {
        let mut table = null_mut();
        // SAFETY: table is a writable pointer slot for allocation by GetIfTable2.
        if unsafe { GetIfTable2(&mut table) } != 0 || table.is_null() {
            return Vec::new();
        }
        let table = Table(table);
        // SAFETY: The returned C table has NumEntries rows starting at the aligned
        // Table member. The allocation remains live for the whole iteration.
        let (entries, first) = unsafe {
            (
                (*table.0).NumEntries,
                addr_of!((*table.0).Table).cast::<MIB_IF_ROW2>(),
            )
        };
        let mut adapters = Vec::new();
        for index in 0..entries.min(128) as usize {
            // SAFETY: index is within the returned array of C rows.
            let row = unsafe { &*first.add(index) };
            let kind = match row.Type {
                IF_TYPE_ETHERNET_CSMACD => "ethernet",
                IF_TYPE_IEEE80211 => "wifi",
                _ => continue,
            };
            // The first bit of InterfaceAndOperStatusFlags is HardwareInterface.
            if row.InterfaceAndOperStatusFlags._bitfield & 1 == 0 {
                continue;
            }
            let state = if row.OperStatus == IfOperStatusUp {
                Some("up")
            } else if [IfOperStatusDown, IfOperStatusLowerLayerDown].contains(&row.OperStatus) {
                Some("down")
            } else if row.OperStatus == IfOperStatusDormant {
                Some("dormant")
            } else if row.OperStatus == IfOperStatusTesting {
                Some("testing")
            } else {
                None
            };
            let speed =
                |value| (state == Some("up") && value > 0 && value != u64::MAX).then_some(value);
            let name = row
                .Description
                .split(|unit| *unit == 0)
                .next()
                .unwrap_or_default();
            adapters.push((
                row.InterfaceIndex,
                Adapter {
                    name: String::from_utf16_lossy(name),
                    kind,
                    state,
                    rx_speed: speed(row.ReceiveLinkSpeed),
                    tx_speed: speed(row.TransmitLinkSpeed),
                    received: Some(row.InOctets),
                    sent: Some(row.OutOctets),
                },
            ));
        }
        adapters.sort_by_key(|(index, _)| *index);
        adapters.into_iter().map(|(_, adapter)| adapter).collect()
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use super::Adapter;
    use std::{fs, path::Path};

    pub(super) fn read(root: &Path) -> Vec<Adapter> {
        let Ok(devices) = fs::read_dir(root) else {
            return Vec::new();
        };
        let mut devices: Vec<_> = devices
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect();
        devices.sort_unstable();
        devices
            .iter()
            .filter_map(|path| {
                let read = |name| {
                    fs::read_to_string(path.join(name))
                        .ok()
                        .map(|data| data.trim().to_owned())
                };
                if !path.join("device").exists() || read("type")?.as_str() != "1" {
                    return None;
                }
                let state = match read("operstate").as_deref() {
                    Some("up") => Some("up"),
                    Some("down" | "lowerlayerdown") => Some("down"),
                    Some("dormant") => Some("dormant"),
                    Some("testing") => Some("testing"),
                    _ => None,
                };
                let number = |name| read(name)?.parse::<u64>().ok();
                let speed = if state == Some("up") {
                    number("speed")
                        .filter(|speed| *speed > 0)
                        .and_then(|speed| speed.checked_mul(1_000_000))
                } else {
                    None
                };
                Some(Adapter {
                    name: path.file_name()?.to_string_lossy().into_owned(),
                    kind: if path.join("wireless").exists() || path.join("phy80211").exists() {
                        "wifi"
                    } else {
                        "ethernet"
                    },
                    state,
                    rx_speed: speed,
                    tx_speed: speed,
                    received: number("statistics/rx_bytes"),
                    sent: number("statistics/tx_bytes"),
                })
            })
            .take(128)
            .collect()
    }

    #[test]
    fn excludes_virtual_interfaces_and_preserves_zero_and_unknown_values() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("eth0");
        fs::create_dir_all(path.join("device")).unwrap();
        fs::create_dir(path.join("statistics")).unwrap();
        for (file, value) in [
            ("type", "1"),
            ("operstate", "up"),
            ("speed", "1000"),
            ("statistics/rx_bytes", "0"),
        ] {
            fs::write(path.join(file), value).unwrap();
        }
        fs::create_dir(root.path().join("virtual")).unwrap();
        let adapters = read(root.path());
        assert_eq!(adapters.len(), 1);
        let adapter = &adapters[0];
        assert_eq!(
            (&*adapter.name, adapter.kind, adapter.state),
            ("eth0", "ethernet", Some("up"))
        );
        assert_eq!(
            (
                adapter.rx_speed,
                adapter.tx_speed,
                adapter.received,
                adapter.sent
            ),
            (Some(1_000_000_000), Some(1_000_000_000), Some(0), None)
        );
        fs::write(path.join("operstate"), "down").unwrap();
        assert_eq!(read(root.path())[0].rx_speed, None);
    }
}
