use super::CpuTicks;
use std::{
    fs,
    io::{self, ErrorKind},
};

pub const CPU_SOURCE: &str = "/proc/cpuinfo";
pub const MEMORY_SOURCE: &str = "/proc/meminfo";
pub const OS_SOURCE: &str = "os-release + /proc/sys/kernel/osrelease";
pub const USAGE_SOURCE: &str = "/proc/stat";

pub fn processor() -> io::Result<String> {
    let info = fs::read_to_string(CPU_SOURCE)?;
    info.lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(key, value)| key.trim() == "model name" && !value.trim().is_empty())
        .map(|(_, value)| value.trim().to_owned())
        .ok_or_else(|| io::Error::from(ErrorKind::Unsupported))
}

pub fn memory_bytes() -> io::Result<u64> {
    let info = fs::read_to_string(MEMORY_SOURCE)?;
    info.lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))
        .and_then(|line| {
            let mut fields = line.split_whitespace();
            let kib = fields.next()?.parse::<u64>().ok()?;
            (fields.next()? == "kB").then_some(())?;
            kib.checked_mul(1024)
        })
        .ok_or_else(|| io::Error::from(ErrorKind::InvalidData))
}

pub fn operating_system() -> io::Result<String> {
    let release = match fs::read_to_string("/etc/os-release") {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            fs::read_to_string("/usr/lib/os-release")
        }
        result => result,
    }?;
    let name = release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|name| name.trim_matches(['"', '\'']))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| io::Error::from(ErrorKind::Unsupported))?;
    let kernel = fs::read_to_string("/proc/sys/kernel/osrelease")?;
    Ok(format!("{name} · {}", kernel.trim()))
}

pub fn cpu_ticks() -> io::Result<CpuTicks> {
    let stat = fs::read_to_string(USAGE_SOURCE)?;
    let aggregate = stat
        .lines()
        .find_map(|line| line.strip_prefix("cpu "))
        .ok_or_else(|| io::Error::from(ErrorKind::InvalidData))?;
    parse_ticks(aggregate)
}

pub(super) fn logical_cpu_ticks() -> io::Result<Vec<(u32, CpuTicks)>> {
    let stat = fs::read_to_string(USAGE_SOURCE)?;
    let mut rows = stat
        .lines()
        .filter_map(|line| {
            let (name, fields) = line.split_once(' ')?;
            let logical = name.strip_prefix("cpu")?.parse().ok()?;
            Some((logical, parse_ticks(fields).ok()?))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable_by_key(|(logical, _)| *logical);
    rows.truncate(64);
    Ok(rows)
}

fn parse_ticks(aggregate: &str) -> io::Result<CpuTicks> {
    let mut ticks = [0_u64; 8];
    let mut fields = aggregate.split_whitespace();
    for (index, tick) in ticks.iter_mut().enumerate() {
        match fields.next() {
            Some(field) => *tick = field.parse().map_err(|_| ErrorKind::InvalidData)?,
            None if index >= 4 => break,
            None => return Err(ErrorKind::InvalidData.into()),
        }
    }
    // Linux includes guest time in user/nice. I/O wait is grouped with idle.
    let [user, nice, system, idle, iowait, irq, softirq, steal] = ticks;
    let sum = |values: &[u64]| {
        values
            .iter()
            .try_fold(0_u64, |total, value| total.checked_add(*value))
            .ok_or_else(|| io::Error::from(ErrorKind::InvalidData))
    };
    Ok(CpuTicks {
        busy: sum(&[user, nice, system, irq, softirq, steal])?,
        idle: sum(&[idle, iowait])?,
    })
}
