use nucelvo_core::{Category as C, Field, Reading, Sample, State};
use std::{
    collections::BTreeSet,
    ffi::CString,
    fs, io,
    mem::MaybeUninit,
    path::{Path, PathBuf},
};

fn read(path: impl AsRef<Path>) -> io::Result<String> {
    fs::read_to_string(path).map(|text| text.trim().to_owned())
}

fn entries(path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort_unstable();
    Ok(paths)
}

fn attr(
    category: C,
    device: usize,
    key: &'static str,
    label: (&'static str, &'static str),
    path: impl AsRef<Path>,
    source: &'static str,
) -> Field {
    let reading = Reading::from_result(read(path), source);
    let mut field = Field::text(category, device, key, label, reading.value, source);
    if reading.state != State::Ok {
        field.state = reading.state;
    }
    field
}

fn num(
    category: C,
    device: usize,
    key: &'static str,
    label: (&'static str, &'static str),
    value: Option<u64>,
    unit: &'static str,
    source: &'static str,
) -> Field {
    Field::text(category, device, key, label, None, source).number(value, unit)
}

pub fn hardware_details() -> Vec<Field> {
    let mut fields = Vec::new();
    const CPU: &str = "/proc/cpuinfo";
    let cpuinfo = read(CPU).unwrap_or_default();
    let vendor = cpuinfo
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, _)| key.trim() == "vendor_id")
        .map(|(_, value)| value.trim().to_owned());
    fields.push(Field::text(
        C::Cpu,
        0,
        "manufacturer",
        ("Manufacturer", "Fabricante"),
        vendor,
        CPU,
    ));
    let cores = entries("/sys/devices/system/cpu")
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| read(path.join("topology/core_cpus_list")).ok())
        .collect::<BTreeSet<_>>()
        .len();
    fields.push(num(
        C::Cpu,
        0,
        "physical_cores",
        ("Physical cores", "Núcleos físicos"),
        (cores > 0).then_some(cores as u64),
        "",
        "sysfs · CPU topology",
    ));
    fields.push(num(
        C::Cpu,
        0,
        "base_clock",
        ("Base clock", "Clock base"),
        read("/sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .and_then(|value| value.checked_mul(1000)),
        "Hz",
        "sysfs · cpufreq",
    ));
    for (index, path) in entries("/sys/devices/system/cpu/cpu0/cache")
        .unwrap_or_default()
        .into_iter()
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("index"))
        })
        .enumerate()
    {
        let value = read(path.join("level"))
            .ok()
            .zip(read(path.join("type")).ok())
            .zip(read(path.join("size")).ok())
            .map(|((level, kind), size)| format!("L{level} · {kind} · {size}"));
        fields.push(Field::text(
            C::Cpu,
            index + 1,
            "cache",
            ("CPU cache", "Cache da CPU"),
            value,
            "sysfs · CPU cache",
        ));
    }
    for (key, label, file) in [
        (
            "manufacturer",
            ("Manufacturer", "Fabricante"),
            "board_vendor",
        ),
        ("model", ("Board model", "Modelo da placa"), "board_name"),
        (
            "firmware_version",
            ("Firmware version", "Versão do firmware"),
            "bios_version",
        ),
        (
            "serial",
            ("Board serial number", "Número de série da placa"),
            "board_serial",
        ),
    ] {
        let mut field = attr(
            C::Board,
            0,
            key,
            label,
            Path::new("/sys/class/dmi/id").join(file),
            "sysfs · DMI",
        );
        field.private = key == "serial";
        fields.push(field);
    }
    memory_slots(&mut fields);
    graphics(&mut fields);
    storage(&mut fields);
    fields.push(num(
        C::System,
        0,
        "uptime",
        ("Uptime at capture", "Tempo ligado na captura"),
        read("/proc/uptime").ok().and_then(|line| {
            line.split_whitespace()
                .next()?
                .split('.')
                .next()?
                .parse()
                .ok()
        }),
        "s",
        "/proc/uptime",
    ));
    fields
}

fn memory_slots(fields: &mut Vec<Field>) {
    const SOURCE: &str = "sysfs · SMBIOS";
    let result = fs::read("/sys/firmware/dmi/tables/DMI");
    let mut state = State::Unsupported;
    if let Ok(bytes) = &result {
        let mut start = 0;
        let mut slot = 1;
        while let Some(header) = bytes.get(start..start + 4) {
            let size = usize::from(header[1]);
            if size < 4 {
                break;
            }
            let Some(record) = bytes.get(start..start + size) else {
                break;
            };
            let Some(strings) = bytes.get(start + size..) else {
                break;
            };
            let Some(end) = strings.windows(2).position(|pair| pair == [0, 0]) else {
                break;
            };
            if header[0] == 17 && size >= 27 {
                let raw = u16::from_le_bytes([record[12], record[13]]);
                if raw != 0 {
                    let bytes = match raw {
                        0xffff => None,
                        0x7fff => record
                            .get(28..32)
                            .and_then(|v| v.try_into().ok())
                            .map(u32::from_le_bytes)
                            .map(|v| u64::from(v & 0x7fffffff) * 1024 * 1024),
                        raw if raw & 0x8000 != 0 => Some(u64::from(raw & 0x7fff) * 1024),
                        raw => Some(u64::from(raw) * 1024 * 1024),
                    };
                    let locator = usize::from(record[16])
                        .checked_sub(1)
                        .and_then(|index| strings[..end].split(|byte| *byte == 0).nth(index))
                        .map(|text| String::from_utf8_lossy(text).into_owned());
                    fields.push(Field::text(
                        C::Memory,
                        slot,
                        "slot",
                        ("Memory slot", "Slot de memória"),
                        locator,
                        SOURCE,
                    ));
                    fields.push(num(
                        C::Memory,
                        slot,
                        "capacity",
                        ("Module capacity", "Capacidade do módulo"),
                        bytes,
                        "bytes",
                        SOURCE,
                    ));
                    fields.push(num(
                        C::Memory,
                        slot,
                        "memory_type",
                        ("SMBIOS memory type", "Tipo de memória SMBIOS"),
                        Some(record[18].into()),
                        "",
                        SOURCE,
                    ));
                    let speed = u16::from_le_bytes([record[21], record[22]]);
                    fields.push(num(
                        C::Memory,
                        slot,
                        "rated_speed",
                        ("Rated speed", "Velocidade nominal"),
                        (speed != 0 && speed != 0xffff).then_some(speed.into()),
                        "MT/s",
                        SOURCE,
                    ));
                    slot += 1;
                }
            }
            if header[0] == 127 {
                break;
            }
            start += size + end + 2;
        }
        if slot > 1 {
            return;
        }
    } else if let Err(error) = result {
        state = Reading::<()>::from_result(Err(error), SOURCE).state;
    }
    let mut field = Field::text(
        C::Memory,
        1,
        "slot",
        ("Memory modules", "Módulos de memória"),
        None,
        SOURCE,
    );
    field.state = state;
    fields.push(field);
}

fn graphics(fields: &mut Vec<Field>) {
    const SOURCE: &str = "sysfs · DRM / PCI";
    let mut devices: BTreeSet<_> = entries("/sys/class/drm")
        .unwrap_or_default()
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_prefix("card"))
                .is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
                })
        })
        .filter_map(|path| fs::canonicalize(path.join("device")).ok())
        .collect();
    for path in entries("/sys/bus/pci/devices").unwrap_or_default() {
        if read(path.join("class")).is_ok_and(|class| class.starts_with("0x03"))
            && let Ok(device) = fs::canonicalize(path)
        {
            devices.insert(device);
        }
    }
    let pci_ids = ["/usr/share/misc/pci.ids", "/usr/share/hwdata/pci.ids"]
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok());
    for (index, device) in devices.iter().enumerate() {
        let codes = read(device.join("vendor"))
            .ok()
            .zip(read(device.join("device")).ok());
        let ids = codes
            .as_ref()
            .map(|(vendor, model)| format!("{vendor}:{model}"));
        let model = codes.as_ref().and_then(|(vendor, model)| {
            pci_ids
                .as_deref()
                .and_then(|ids| pci_name(ids, vendor, model))
        });
        if let Some(model) = model {
            fields.push(Field::text(
                C::Graphics,
                index,
                "model",
                ("Adapter", "Adaptador"),
                Some(model),
                "PCI IDs · local database",
            ));
        }
        fields.push(Field::text(
            C::Graphics,
            index,
            "pci_id",
            ("Adapter PCI ID", "ID PCI do adaptador"),
            ids,
            SOURCE,
        ));
        let driver = fs::read_link(device.join("driver")).ok().and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        });
        fields.push(Field::text(
            C::Graphics,
            index,
            "driver",
            ("Driver", "Driver"),
            driver,
            SOURCE,
        ));
        fields.push(num(
            C::Graphics,
            index,
            "dedicated_vram",
            ("Dedicated VRAM", "VRAM dedicada"),
            read(device.join("mem_info_vram_total"))
                .ok()
                .and_then(|v| v.parse().ok()),
            "bytes",
            SOURCE,
        ));
    }
    if devices.is_empty() {
        fields.push(Field::text(
            C::Graphics,
            0,
            "adapter",
            ("Adapter", "Adaptador"),
            None,
            SOURCE,
        ));
    }
}

fn pci_name(database: &str, vendor: &str, model: &str) -> Option<String> {
    let vendor = vendor.strip_prefix("0x")?;
    let model = model.strip_prefix("0x")?;
    let mut vendor_matches = false;
    for line in database.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.starts_with('\t') {
            vendor_matches = line.split_whitespace().next() == Some(vendor);
        } else if vendor_matches
            && let Some(entry) = line
                .strip_prefix('\t')
                .filter(|line| !line.starts_with('\t'))
            && let Some((id, name)) = entry.split_once(' ')
            && id == model
        {
            return Some(name.trim().to_owned());
        }
    }
    None
}

fn storage(fields: &mut Vec<Field>) {
    const SOURCE: &str = "sysfs · block";
    let health = super::storage_health::linux::Probe::new();
    let mut count = 0;
    let mut physical_paths = Vec::new();
    for path in entries("/sys/block").unwrap_or_default() {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
            continue;
        }
        let device = path.join("device");
        fields.push(attr(
            C::Storage,
            count,
            "model",
            ("Disk model", "Modelo do disco"),
            device.join("model"),
            SOURCE,
        ));
        let capacity = read(path.join("size"))
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .and_then(|v| v.checked_mul(512));
        fields.push(num(
            C::Storage,
            count,
            "capacity",
            ("Disk capacity", "Capacidade do disco"),
            capacity,
            "bytes",
            SOURCE,
        ));
        let interface = fs::canonicalize(&device).ok().and_then(|path| {
            let path = path.to_string_lossy();
            ["nvme", "usb", "virtio", "ata", "mmc"]
                .into_iter()
                .find(|family| path.split('/').any(|part| part.starts_with(family)))
                .map(str::to_uppercase)
        });
        fields.push(Field::text(
            C::Storage,
            count,
            "interface",
            ("Interface", "Interface"),
            interface,
            SOURCE,
        ));
        if device.exists() {
            if let Ok(canonical) = path.canonicalize() {
                physical_paths.push((canonical, count));
            }
            fields.extend(health.collect(count, name));
        }
        count += 1;
    }
    if count == 0 {
        fields.push(Field::text(
            C::Storage,
            0,
            "disk",
            ("Disk", "Disco"),
            None,
            SOURCE,
        ));
    }
    let mounts = read("/proc/self/mountinfo").unwrap_or_default();
    for (index, (mount, kind, block)) in mounts
        .lines()
        .filter_map(|line| {
            let (head, tail) = line.split_once(" - ")?;
            let mut tail = tail.split_whitespace();
            let kind = tail.next()?;
            let origin = tail.next()?;
            if !origin.starts_with("/dev/") {
                return None;
            }
            let mut head = head.split_whitespace();
            let block = head.nth(2)?;
            Some((head.nth(1)?, kind, block))
        })
        .take(256)
        .enumerate()
    {
        const MOUNT: &str = "/proc/self/mountinfo · statvfs";
        let device = 10_000 + index;
        let first_field = fields.len();
        let mount = mount
            .replace("\\040", " ")
            .replace("\\011", "\t")
            .replace("\\012", "\n")
            .replace("\\134", "\\");
        let sizes = volume_sizes(&mount);
        let mut field = Field::text(
            C::Storage,
            device,
            "mount_point",
            ("Mount point", "Ponto de montagem"),
            Some(mount),
            MOUNT,
        );
        field.private = true;
        fields.push(field);
        fields.push(Field::text(
            C::Storage,
            device,
            "filesystem",
            ("File system", "Sistema de arquivos"),
            Some(kind.into()),
            MOUNT,
        ));
        fields.push(num(
            C::Storage,
            device,
            "volume_capacity",
            ("Volume capacity", "Capacidade do volume"),
            sizes.map(|(total, _)| total),
            "bytes",
            MOUNT,
        ));
        fields.push(num(
            C::Storage,
            device,
            "free_space",
            ("Free space", "Espaço livre"),
            sizes.map(|(_, free)| free),
            "bytes",
            MOUNT,
        ));
        if let Ok(path) = Path::new("/sys/dev/block").join(block).canonicalize()
            && let Some((_, parent)) = physical_paths
                .iter()
                .find(|(disk, _)| path.starts_with(disk))
        {
            for field in &mut fields[first_field..] {
                field.storage_parent = Some(*parent);
            }
        }
    }
}

fn volume_sizes(path: &str) -> Option<(u64, u64)> {
    let path = CString::new(path).ok()?;
    let mut stats = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is NUL-terminated and stats points to writable storage.
    if unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: statvfs initialized the output when it returned success.
    let stats = unsafe { stats.assume_init() };
    Some((
        stats.f_frsize.checked_mul(stats.f_blocks)?,
        stats.f_frsize.checked_mul(stats.f_bavail)?,
    ))
}

pub fn memory_load() -> io::Result<f64> {
    let text = read("/proc/meminfo")?;
    let amount = |key: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|line| line.split_whitespace().next()?.parse::<u64>().ok())
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))
    };
    let total = amount("MemTotal:")?;
    let available = amount("MemAvailable:")?;
    if total == 0 || available > total {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(100.0 * (total - available) as f64 / total as f64)
}

pub fn sensors() -> Vec<Sample> {
    let mut samples = cpu_frequencies(Path::new("/sys/devices/system/cpu/cpufreq"));
    samples.extend(hwmon_sensors());
    samples
}

fn cpu_frequencies(root: &Path) -> Vec<Sample> {
    let mut policies: Vec<_> = entries(root)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| {
            let index = path
                .file_name()?
                .to_str()?
                .strip_prefix("policy")?
                .parse::<u32>()
                .ok()?;
            Some((index, path))
        })
        .collect();
    policies.sort_unstable_by_key(|(index, _)| *index);
    policies
        .into_iter()
        .take(16)
        .map(|(index, path)| {
            let hardware = read(path.join("cpuinfo_cur_freq"));
            let (value, kind, source, en, pt) = if hardware
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
            {
                (
                    read(path.join("scaling_cur_freq")),
                    "driver",
                    "sysfs · CPUFreq scaling_cur_freq",
                    "Driver-reported CPU frequency",
                    "Frequência da CPU informada pelo driver",
                )
            } else {
                (
                    hardware,
                    "hardware",
                    "sysfs · CPUFreq cpuinfo_cur_freq",
                    "Hardware CPU frequency",
                    "Frequência da CPU medida pelo hardware",
                )
            };
            let hertz = value.and_then(|text| {
                text.parse::<u64>()
                    .ok()
                    .filter(|value| *value > 0)
                    .and_then(|value| value.checked_mul(1000))
                    .map(|value| value as f64)
                    .ok_or_else(|| io::ErrorKind::InvalidData.into())
            });
            let mut sample = super::sample(
                format!("cpufreq:{index}:{kind}"),
                format!("{en} · Policy {index}"),
                None,
                "Hz",
                Reading::from_result(hertz, source),
            );
            sample.name_pt = Some(format!("{pt} · Política {index}").into());
            sample
        })
        .collect()
}

fn hwmon_sensors() -> Vec<Sample> {
    const SOURCE: &str = "Linux · hwmon";
    let paths = match entries("/sys/class/hwmon") {
        Ok(paths) => paths,
        Err(error) => {
            return super::unavailable_sensor(
                SOURCE,
                Reading::<f64>::from_result(Err(error), SOURCE).state,
            );
        }
    };
    let mut samples = Vec::new();
    for path in paths {
        let chip = read(path.join("name")).unwrap_or_else(|_| "hwmon".into());
        let device = fs::canonicalize(path.join("device"))
            .or_else(|_| {
                fs::canonicalize(&path)
                    .map(|resolved| resolved.parent().map(Path::to_path_buf).unwrap_or(resolved))
            })
            .unwrap_or_else(|_| path.clone());
        for channel in entries(&path).unwrap_or_default() {
            let Some(stem) = channel
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix("_input"))
            else {
                continue;
            };
            let clock = match (chip.as_str(), stem) {
                ("amdgpu", "freq1") => Some(("GPU graphics clock", "Clock gráfico da GPU")),
                ("amdgpu", "freq2") => Some(("GPU memory clock", "Clock da memória da GPU")),
                _ => None,
            };
            let quantity = [
                ("temp", "°C", 1000.0),
                ("fan", "RPM", 1.0),
                ("in", "V", 1000.0),
                ("power", "W", 1_000_000.0),
            ]
            .into_iter()
            .find(|(prefix, _, _)| {
                stem.strip_prefix(prefix).is_some_and(|index| {
                    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())
                })
            })
            .map(|(_, unit, divisor)| (unit, divisor));
            let Some((unit, divisor)) = quantity.or(clock.map(|_| ("Hz", 1.0))) else {
                continue;
            };
            let label = read(path.join(format!("{stem}_label"))).unwrap_or_else(|_| stem.into());
            let value = read(&channel)
                .and_then(|text| {
                    text.parse::<i64>()
                        .map_err(|_| io::ErrorKind::InvalidData.into())
                })
                .map(|value| value as f64 / divisor);
            let mut sample = super::sample(
                format!("hwmon:{}:{chip}:{stem}", device.display()),
                clock.map_or_else(
                    || format!("{chip} · {label}"),
                    |(en, _)| format!("{en} · {chip}"),
                ),
                None,
                unit,
                Reading::from_result(value, SOURCE),
            );
            if let Some((_, pt)) = clock {
                sample.name_pt = Some(format!("{pt} · {chip}").into());
            }
            samples.push(sample);
        }
    }
    if samples.is_empty() {
        return super::unavailable_sensor(SOURCE, State::Unsupported);
    }
    samples
}

#[cfg(test)]
mod frequency_tests {
    use super::*;

    #[test]
    fn keeps_measured_and_driver_reported_frequencies_distinct() {
        let root = tempfile::tempdir().unwrap();
        let hardware = root.path().join("policy2");
        let driver = root.path().join("policy10");
        fs::create_dir(&hardware).unwrap();
        fs::create_dir(&driver).unwrap();
        fs::write(hardware.join("cpuinfo_cur_freq"), "2430000\n").unwrap();
        fs::write(hardware.join("scaling_cur_freq"), "1300000\n").unwrap();
        fs::write(driver.join("scaling_cur_freq"), "1900000\n").unwrap();

        let samples = cpu_frequencies(root.path());
        assert_eq!(samples.len(), 2);
        assert_eq!(
            (&*samples[0].id, samples[0].value),
            ("cpufreq:2:hardware", Some(2.43e9))
        );
        assert_eq!(
            (&*samples[1].id, samples[1].value),
            ("cpufreq:10:driver", Some(1.9e9))
        );
        fs::write(hardware.join("cpuinfo_cur_freq"), "invalid").unwrap();
        assert_eq!(cpu_frequencies(root.path())[0].value, None);
    }
}
