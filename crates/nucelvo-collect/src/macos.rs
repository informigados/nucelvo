use std::{
    ffi::CStr,
    io::{self, ErrorKind},
    mem::{offset_of, size_of},
    ptr, slice,
};

use mach2::{mach_init::mach_host_self, mach_port::mach_port_deallocate, traps::mach_task_self};
use nucelvo_core::{Category, Field, Reading};

use super::CpuTicks;

pub const CPU_SOURCE: &str = "macOS · sysctl machdep.cpu.brand_string";
pub const MEMORY_SOURCE: &str = "macOS · sysctl hw.memsize";
pub const MEMORY_LOAD_SOURCE: &str = "macOS · Mach VM statistics";
pub const OS_SOURCE: &str = "macOS · sysctl";
pub const USAGE_SOURCE: &str = "macOS · Mach CPU ticks";

struct HostPort(libc::mach_port_t);

impl HostPort {
    fn open() -> Self {
        // mach_host_self returns a send right owned by the calling task.
        Self(unsafe { mach_host_self() })
    }
}

impl Drop for HostPort {
    fn drop(&mut self) {
        // Release the send right after each query, including failed queries.
        unsafe { mach_port_deallocate(mach_task_self(), self.0) };
    }
}

fn mach_result(status: libc::kern_return_t) -> io::Result<()> {
    if status == libc::KERN_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::other(format!("Mach returned {status}")))
    }
}

fn sysctl(name: &CStr, buffer: &mut [u8]) -> io::Result<usize> {
    let mut size = buffer.len();
    // The supplied writable buffer is valid for size bytes. A null new value makes this read-only.
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    if size > buffer.len() {
        return Err(ErrorKind::InvalidData.into());
    }
    Ok(size)
}

fn sysctl_string(name: &CStr) -> io::Result<String> {
    let mut buffer = [0; 256];
    let size = sysctl(name, &mut buffer)?;
    let text = CStr::from_bytes_until_nul(&buffer[..size])
        .map_err(|_| ErrorKind::InvalidData)?
        .to_str()
        .map_err(|_| ErrorKind::InvalidData)?
        .trim();
    if text.is_empty() {
        return Err(ErrorKind::InvalidData.into());
    }
    Ok(text.to_owned())
}

pub fn processor() -> io::Result<String> {
    sysctl_string(c"machdep.cpu.brand_string")
}

pub fn memory_bytes() -> io::Result<u64> {
    let mut bytes = [0; size_of::<u64>()];
    let size = sysctl(c"hw.memsize", &mut bytes)?;
    let total = u64::from_ne_bytes(bytes);
    if size != bytes.len() || total == 0 {
        return Err(ErrorKind::InvalidData.into());
    }
    Ok(total)
}

pub fn operating_system() -> io::Result<String> {
    let version = sysctl_string(c"kern.osproductversion")?;
    let build = sysctl_string(c"kern.osversion")?;
    Ok(format!("macOS {version} ({build})"))
}

fn cpu_states([user, system, idle, nice]: [u32; 4]) -> CpuTicks {
    CpuTicks {
        busy: u64::from(user) + u64::from(system) + u64::from(nice),
        idle: u64::from(idle),
    }
}

pub fn cpu_ticks() -> io::Result<CpuTicks> {
    let host = HostPort::open();
    let mut info = libc::host_cpu_load_info { cpu_ticks: [0; 4] };
    let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
    // The flavor returns one host_cpu_load_info, with count in Mach integers.
    mach_result(unsafe {
        libc::host_statistics(
            host.0,
            libc::HOST_CPU_LOAD_INFO,
            ptr::from_mut(&mut info).cast(),
            &mut count,
        )
    })?;
    if count != libc::HOST_CPU_LOAD_INFO_COUNT {
        return Err(ErrorKind::InvalidData.into());
    }
    Ok(cpu_states(info.cpu_ticks))
}

pub(super) fn logical_cpu_ticks() -> io::Result<Vec<(u32, CpuTicks)>> {
    let host = HostPort::open();
    let mut processors = 0;
    let mut words = 0;
    let mut info = ptr::null_mut();
    // The kernel allocates the returned array in this task's address space.
    mach_result(unsafe {
        libc::host_processor_info(
            host.0,
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut processors,
            &mut info,
            &mut words,
        )
    })?;
    if info.is_null() {
        return Err(ErrorKind::InvalidData.into());
    }

    let word_count = words as usize;
    let rows = if processors != 0 && processors as usize * 4 == word_count {
        // Each processor has four Mach integers carrying unsigned tick counts.
        unsafe { slice::from_raw_parts(info, word_count) }
            .as_chunks::<4>()
            .0
            .iter()
            .take(64)
            .enumerate()
            .map(|(index, ticks)| (index as u32, cpu_states(ticks.map(|tick| tick as u32))))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    // Return the kernel-allocated array even if its reported shape was unexpected.
    mach_result(unsafe {
        libc::vm_deallocate(
            mach_task_self(),
            info as libc::vm_address_t,
            word_count * size_of::<libc::integer_t>(),
        )
    })?;
    if rows.is_empty() {
        return Err(ErrorKind::InvalidData.into());
    }
    Ok(rows)
}

pub fn memory_load() -> io::Result<f64> {
    let host = HostPort::open();
    // All fields in the C layout are integer counters. Zero also initializes fields
    // omitted by older macOS kernels that return a shorter version of this structure.
    let mut info: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    mach_result(unsafe {
        libc::host_statistics64(
            host.0,
            libc::HOST_VM_INFO64,
            ptr::from_mut(&mut info).cast(),
            &mut count,
        )
    })?;
    let required = offset_of!(libc::vm_statistics64, internal_page_count) + size_of::<u32>();
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if count as usize * size_of::<libc::integer_t>() < required || page_size <= 0 {
        return Err(ErrorKind::InvalidData.into());
    }
    let pages = (u64::from(info.internal_page_count)
        + u64::from(info.wire_count)
        + u64::from(info.compressor_page_count))
    .saturating_sub(u64::from(info.purgeable_count));
    Ok((100.0 * pages as f64 * page_size as f64 / memory_bytes()? as f64).min(100.0))
}

pub fn hardware_details() -> Vec<Field> {
    let mut uptime = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // Darwin's raw monotonic clock counts elapsed time since boot, including sleep.
    let elapsed = if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC_RAW, &mut uptime) } == 0 {
        u64::try_from(uptime.tv_sec).map_err(|_| ErrorKind::InvalidData.into())
    } else {
        Err(io::Error::last_os_error())
    };
    let elapsed = Reading::from_result(elapsed, "macOS · monotonic clock");
    let mut uptime = Field::text(
        Category::System,
        0,
        "uptime",
        ("Uptime at capture", "Tempo ligado na captura"),
        None,
        elapsed.source,
    )
    .number(elapsed.value, "s");
    uptime.state = elapsed.state;

    let model = Reading::from_result(sysctl_string(c"hw.model"), "macOS · sysctl hw.model");
    let mut machine = Field::text(
        Category::System,
        0,
        "hardware_model",
        ("Hardware model", "Modelo do hardware"),
        model.value,
        model.source,
    );
    machine.state = model.state;

    let unavailable = |category, key, labels| Field::text(category, 0, key, labels, None, "macOS");
    vec![
        uptime,
        machine,
        unavailable(Category::Board, "board_model", ("Motherboard", "Placa-mãe")),
        unavailable(
            Category::Graphics,
            "adapter",
            ("Graphics adapter", "Adaptador de vídeo"),
        ),
        unavailable(
            Category::Storage,
            "drive_model",
            ("Storage device", "Unidade de armazenamento"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_native_inventory_and_usage_counters() -> io::Result<()> {
        assert!(!processor()?.is_empty());
        assert!(operating_system()?.starts_with("macOS "));
        assert!(memory_bytes()? > 0);
        assert!((0.0..=100.0).contains(&memory_load()?));
        let aggregate = cpu_ticks()?;
        assert!(aggregate.busy + aggregate.idle > 0);
        assert!(!logical_cpu_ticks()?.is_empty());
        Ok(())
    }
}
