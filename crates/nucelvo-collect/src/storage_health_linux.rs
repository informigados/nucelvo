use std::{
    fs,
    io::{self, Read},
    os::unix::fs::FileTypeExt,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use nucelvo_core::{Field, State};
use serde_json::Value;

use super::{Health, data_bytes, fields};

const SOURCE: &str = "smartmontools · smartctl";
const MAX_OUTPUT: u64 = 256 * 1024;

pub(in crate::details) struct Probe {
    command: Option<&'static Path>,
    deadline: Instant,
}

impl Probe {
    pub(in crate::details) fn new() -> Self {
        let command = ["/usr/sbin/smartctl", "/usr/bin/smartctl", "/sbin/smartctl"]
            .into_iter()
            .map(Path::new)
            .find(|path| path.is_file());
        Self {
            command,
            deadline: Instant::now() + Duration::from_secs(3),
        }
    }

    pub(in crate::details) fn collect(&self, device: usize, name: &str) -> Vec<Field> {
        let path = Path::new("/dev").join(name);
        let result = self.command.ok_or(State::Unsupported).and_then(|command| {
            if !fs::metadata(&path).is_ok_and(|metadata| metadata.file_type().is_block_device()) {
                return Err(State::Unsupported);
            }
            if Instant::now() >= self.deadline {
                return Err(State::TemporarilyUnavailable);
            }
            read(command, &path, self.deadline)
        });
        fields(device, SOURCE, result)
    }
}

fn read(command: &Path, device: &Path, deadline: Instant) -> Result<Health, State> {
    let mut child = Command::new(command)
        .args(["-j", "-H", "-A", "-n", "standby,0", "--"])
        .arg(device)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| match error.kind() {
            io::ErrorKind::PermissionDenied => State::PermissionDenied,
            io::ErrorKind::NotFound => State::Unsupported,
            _ => State::Error,
        })?;
    let stdout = child.stdout.take().expect("smartctl stdout is piped");
    let output = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
        Ok::<_, io::Error>(bytes)
    });
    let exit = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Ok(exit),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            pending => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(if pending.is_ok() {
                    State::TemporarilyUnavailable
                } else {
                    State::Error
                });
            }
        }
    };
    let bytes = output
        .join()
        .map_err(|_| State::Error)?
        .map_err(|_| State::Error)?;
    let exit = exit?.code().ok_or(State::Error)?;
    if bytes.len() as u64 > MAX_OUTPUT {
        return Err(State::Error);
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|_| State::Error)?;
    decode(&report, exit)
}

fn decode(report: &Value, exit: i32) -> Result<Health, State> {
    if exit & 1 != 0 {
        return Err(State::Error);
    }
    if exit & 2 != 0 {
        let denied = report["smartctl"]["messages"]
            .as_array()
            .is_some_and(|messages| {
                messages.iter().any(|message| {
                    message["string"]
                        .as_str()
                        .is_some_and(|text| text.contains("Permission denied"))
                })
            });
        return Err(if denied {
            State::PermissionDenied
        } else {
            State::Unsupported
        });
    }
    let nvme = &report["nvme_smart_health_information_log"];
    let ata = |name| {
        report["ata_smart_attributes"]["table"]
            .as_array()?
            .iter()
            .find(|row| row["name"].as_str() == Some(name))?["raw"]["value"]
            .as_u64()
    };
    let health = Health {
        passed: report["smart_status"]["passed"].as_bool().or_else(|| {
            nvme["critical_warning"]
                .as_u64()
                .map(|warnings| warnings == 0)
        }),
        hours: report["power_on_time"]["hours"]
            .as_u64()
            .or_else(|| nvme["power_on_hours"].as_u64()),
        cycles: report["power_cycle_count"]
            .as_u64()
            .or_else(|| nvme["power_cycles"].as_u64()),
        wear: nvme["percentage_used"].as_u64(),
        temperature: report["temperature"]["current"]
            .as_u64()
            .or_else(|| nvme["temperature"].as_u64())
            .filter(|value| *value <= 150),
        spare: nvme["available_spare"]
            .as_u64()
            .filter(|value| *value <= 100),
        media_errors: nvme["media_errors"].as_u64(),
        unsafe_shutdowns: nvme["unsafe_shutdowns"].as_u64(),
        bytes_read: nvme["data_units_read"].as_u64().and_then(data_bytes),
        bytes_written: nvme["data_units_written"].as_u64().and_then(data_bytes),
        reallocated: ata("Reallocated_Sector_Ct"),
        pending: ata("Current_Pending_Sector"),
        uncorrectable: ata("Offline_Uncorrectable"),
    };
    Ok(health)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn smartctl_reports_alerts_even_when_exit_status_is_nonzero() {
        let report = json!({
            "smart_status": {"passed": false},
            "power_on_time": {"hours": 123},
            "nvme_smart_health_information_log": {
                "percentage_used": 105, "data_units_written": 10, "available_spare": 100
            }
        });
        let health = decode(&report, 8).unwrap();
        assert_eq!(health.passed, Some(false));
        assert_eq!(health.hours, Some(123));
        assert_eq!(health.wear, Some(105));
        assert_eq!(health.bytes_written, Some(5_120_000));
        assert!(matches!(decode(&report, 2), Err(State::Unsupported)));
    }
}
