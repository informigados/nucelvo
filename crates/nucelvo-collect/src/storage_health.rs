use nucelvo_core::{Category, Field, State};

#[cfg(target_os = "linux")]
#[path = "storage_health_linux.rs"]
pub(super) mod linux;
#[cfg(windows)]
#[path = "storage_health_windows.rs"]
pub(super) mod windows;

#[derive(Debug, Default)]
struct Health {
    passed: Option<bool>,
    hours: Option<u64>,
    cycles: Option<u64>,
    wear: Option<u64>,
    temperature: Option<u64>,
    spare: Option<u64>,
    media_errors: Option<u64>,
    unsafe_shutdowns: Option<u64>,
    bytes_read: Option<u64>,
    bytes_written: Option<u64>,
    reallocated: Option<u64>,
    pending: Option<u64>,
    uncorrectable: Option<u64>,
}

fn fields(device: usize, source: &'static str, result: Result<Health, State>) -> Vec<Field> {
    let missing = result.as_ref().err().copied().unwrap_or(State::Unsupported);
    let health = result.unwrap_or_default();
    let mut status = Field::text(
        Category::Storage,
        device,
        "drive_health",
        ("Drive health", "Saúde da unidade"),
        health
            .passed
            .map(|passed| if passed { "no_alert" } else { "alert" }.into()),
        source,
    );
    if status.value.is_none() {
        status.state = missing;
    }
    let mut output = vec![status];
    for (key, labels, value, unit) in [
        (
            "power_on_hours",
            ("Power-on hours", "Horas de funcionamento"),
            health.hours,
            "h",
        ),
        (
            "power_cycles",
            ("Power cycles", "Ciclos de energia"),
            health.cycles,
            "",
        ),
        (
            "ssd_endurance_used",
            (
                "Estimated SSD endurance used",
                "Resistência estimada do SSD consumida",
            ),
            health.wear,
            "%",
        ),
        (
            "drive_temperature",
            ("Drive temperature", "Temperatura da unidade"),
            health.temperature,
            "°C",
        ),
        (
            "available_spare",
            ("Available spare capacity", "Reserva disponível"),
            health.spare,
            "%",
        ),
        (
            "media_errors",
            ("Media and data errors", "Erros de mídia e dados"),
            health.media_errors,
            "",
        ),
        (
            "unsafe_shutdowns",
            ("Unsafe shutdowns", "Desligamentos inesperados"),
            health.unsafe_shutdowns,
            "",
        ),
        (
            "storage_bytes_read",
            ("Data read by the host", "Dados lidos pelo sistema"),
            health.bytes_read,
            "bytes",
        ),
        (
            "storage_bytes_written",
            ("Data written by the host", "Dados gravados pelo sistema"),
            health.bytes_written,
            "bytes",
        ),
        (
            "reallocated_sectors",
            ("Reallocated sectors", "Setores realocados"),
            health.reallocated,
            "",
        ),
        (
            "pending_sectors",
            ("Pending sectors", "Setores pendentes"),
            health.pending,
            "",
        ),
        (
            "uncorrectable_sectors",
            ("Uncorrectable sectors", "Setores incorrigíveis"),
            health.uncorrectable,
            "",
        ),
    ] {
        if value.is_some() || key == "power_on_hours" {
            let mut field = Field::text(Category::Storage, device, key, labels, None, source)
                .number(value, unit);
            if value.is_none() {
                field.state = missing;
            }
            output.push(field);
        }
    }
    output
}

fn data_bytes(units: u64) -> Option<u64> {
    // Each NVMe data unit is 1,000 blocks of 512 bytes; zero is unreported.
    (units != 0).then_some(units.checked_mul(512_000)).flatten()
}
