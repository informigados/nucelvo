use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use nucelvo_core::{Category, Field, Sample, State};

#[derive(Default)]
struct Adapter {
    id: Option<u64>,
    name: String,
    kind: &'static str,
    state: Option<&'static str>,
    rx_speed: Option<u64>,
    tx_speed: Option<u64>,
    received: Option<u64>,
    sent: Option<u64>,
}

#[cfg(windows)]
const SOURCE: &str = "Win32 · IP Helper";
#[cfg(target_os = "linux")]
const SOURCE: &str = "sysfs · network";

fn adapters() -> Vec<Adapter> {
    #[cfg(windows)]
    return windows::read();
    #[cfg(target_os = "linux")]
    return linux::read(std::path::Path::new("/sys/class/net"));
}

type Counters = BTreeMap<u64, [Option<u64>; 2]>;

#[derive(Default)]
pub struct NetworkSampler {
    previous: Option<(Instant, Counters)>,
}

impl NetworkSampler {
    pub fn sample(&mut self) -> [Sample; 2] {
        let counters: Counters = adapters()
            .into_iter()
            .filter(|adapter| adapter.state == Some("up"))
            .filter_map(|adapter| Some((adapter.id?, [adapter.received, adapter.sent])))
            .collect();
        let seen = Instant::now();
        let at = chrono::Utc::now();
        let (rates, missing_state) = self.rates(seen, counters);
        let reading = |index: usize, id: &str, name: &str, pt: &str| Sample {
            seen,
            at,
            id: id.into(),
            name: name.into(),
            name_pt: Some(pt.to_owned().into()),
            unit: "bit/s",
            source: SOURCE,
            value: rates[index],
            state: if rates[index].is_some() {
                State::Ok
            } else {
                missing_state
            },
        };
        [
            reading(0, "network:receive", "Receive rate", "Taxa de recepção"),
            reading(1, "network:send", "Send rate", "Taxa de envio"),
        ]
    }

    fn rates(&mut self, now: Instant, counters: Counters) -> ([Option<f64>; 2], State) {
        let old = self.previous.take();
        if counters.is_empty() {
            return ([None; 2], State::Unsupported);
        }
        let mut rates = [None; 2];
        if let Some((previous_at, previous)) = old {
            let elapsed = now.saturating_duration_since(previous_at);
            if !elapsed.is_zero()
                && elapsed <= Duration::from_secs(3)
                && counters.keys().eq(previous.keys())
            {
                rates = [0, 1].map(|direction| {
                    let bytes = counters.iter().try_fold(0.0, |sum, (id, current)| {
                        let delta = current[direction]?.checked_sub(previous[id][direction]?)?;
                        Some(sum + delta as f64)
                    })?;
                    Some(bytes * 8.0 / elapsed.as_secs_f64())
                });
            }
        }
        self.previous = Some((now, counters));
        (rates, State::TemporarilyUnavailable)
    }
}

pub(super) fn fields() -> Vec<Field> {
    let source = SOURCE;
    let mut fields = Vec::new();
    for (index, adapter) in adapters().into_iter().enumerate() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_handle_idle_traffic_resets_and_adapter_changes() {
        let mut sampler = NetworkSampler::default();
        let start = Instant::now();
        let mut read = |seconds, entries: &[(u64, [Option<u64>; 2])]| {
            sampler
                .rates(
                    start + Duration::from_secs(seconds),
                    entries.iter().copied().collect(),
                )
                .0
        };
        assert_eq!(read(0, &[(1, [Some(100), Some(200)])]), [None, None]);
        assert_eq!(
            read(2, &[(1, [Some(1100), Some(200)])]),
            [Some(4000.0), Some(0.0)]
        );
        assert_eq!(read(3, &[(1, [Some(5), Some(300)])]), [None, Some(800.0)]);
        assert_eq!(read(4, &[(2, [Some(9000), None])]), [None, None]);
        assert_eq!(read(5, &[(2, [Some(9100), Some(50)])]), [Some(800.0), None]);
        assert_eq!(read(9, &[(2, [Some(9800), Some(100)])]), [None, None]);
        assert_eq!(
            read(10, &[(2, [Some(9900), Some(110)]), (3, [Some(1), Some(2)])]),
            [None, None]
        );
        assert_eq!(
            read(
                11,
                &[(2, [Some(10000), Some(115)]), (3, [Some(11), Some(7)])]
            ),
            [Some(880.0), Some(80.0)]
        );
        assert_eq!(read(12, &[]), [None, None]);
    }
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
                    // SAFETY: Value is the integer view of the populated LUID.
                    id: Some(unsafe { row.InterfaceLuid.Value }),
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
                    id: number("ifindex"),
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
