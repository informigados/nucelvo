use std::collections::BTreeMap;

use nucelvo_core::{Category, Field, State, Value};
use nucelvo_report::{field_display, source_label};

use crate::Row;

const FIRST_VOLUME: usize = 10_000;

pub fn drive(field: &Field) -> usize {
    if field.device < FIRST_VOLUME {
        field.device
    } else {
        field.storage_parent.unwrap_or(usize::MAX)
    }
}

pub fn drives(fields: &[Field], pt: bool) -> BTreeMap<usize, String> {
    let mut drives = BTreeMap::new();
    for field in fields
        .iter()
        .filter(|field| field.category == Category::Storage)
    {
        let id = drive(field);
        if field.device < FIRST_VOLUME
            && field.key == "model"
            && let Some(Value::Text(name)) = &field.value
        {
            let prefix = if pt { "Unidade" } else { "Drive" };
            drives.insert(id, format!("{prefix} {} · {}", id + 1, name.trim()));
        } else {
            drives.entry(id).or_insert_with(|| {
                if id == usize::MAX {
                    if pt {
                        "Outros volumes"
                    } else {
                        "Other volumes"
                    }
                    .into()
                } else {
                    let prefix = if pt { "Unidade" } else { "Drive" };
                    format!("{prefix} {}", id + 1)
                }
            });
        }
    }
    drives
}

pub fn row(field: &Field, pt: bool) -> Row {
    Row {
        label: field.label(pt).into(),
        value: field_display(field, pt).into(),
        detail: source_label(field.source, pt).into(),
        group: field.group(pt).into(),
        note: note(field.key, pt).into(),
        copyable: field.state == State::Ok,
        warning: field.key == "drive_health"
            && matches!(field.value.as_ref(), Some(Value::Text(value)) if value == "alert")
            || field.key == "ssd_endurance_used"
                && matches!(field.value.as_ref(), Some(Value::Number(value)) if *value >= 100),
        ..Default::default()
    }
}

pub fn subgroup(field: &Field, volume_ordinals: &BTreeMap<usize, Vec<usize>>) -> String {
    if field.device < FIRST_VOLUME {
        return String::new();
    }
    let volumes = &volume_ordinals[&drive(field)];
    if volumes.len() == 1 {
        "Volume".into()
    } else {
        let number = volumes
            .iter()
            .position(|id| *id == field.device)
            .unwrap_or(0)
            + 1;
        format!("Volume {number}")
    }
}

pub fn section(field: &Field) -> u8 {
    if field.device >= FIRST_VOLUME {
        1
    } else if matches!(
        field.source,
        "Windows · NVMe health log" | "Windows · ATA SMART" | "smartmontools · smartctl"
    ) {
        2
    } else {
        0
    }
}

fn note(key: &str, pt: bool) -> &'static str {
    let text = match key {
        "uptime" => (
            "Time since the system started, measured when this inventory was captured. Shown as hours, minutes, and seconds.",
            "Tempo desde a inicialização do sistema até a captura do inventário. Exibido em horas, minutos e segundos.",
        ),
        "dedicated_vram" => (
            "Memory reserved for the graphics adapter. On dedicated cards this is typically the memory built into the card.",
            "Memória reservada para o adaptador gráfico. Em placas dedicadas, geralmente é a memória da própria placa.",
        ),
        "shared_memory" => (
            "System RAM the graphics adapter can borrow as needed. This value represents the available limit.",
            "Parte da RAM que o adaptador gráfico pode usar conforme necessário. O valor representa o limite disponível.",
        ),
        "logical_processors" => (
            "The number of CPU execution threads the operating system can schedule.",
            "Quantidade de threads de execução da CPU que o sistema operacional pode usar.",
        ),
        "physical_cores" => (
            "The number of physical processing cores reported for the CPU.",
            "Quantidade de núcleos físicos de processamento informada para a CPU.",
        ),
        "configured_speed" => (
            "The memory speed configured by the system firmware for this module.",
            "Velocidade configurada pelo firmware do sistema para este módulo de memória.",
        ),
        "interface" => (
            "The interface reported by the operating system. Some Windows storage drivers list an NVMe drive as SCSI.",
            "Interface informada pelo sistema operacional. Alguns drivers de armazenamento do Windows listam unidades NVMe como SCSI.",
        ),
        "power_on_hours" => (
            "The cumulative time the drive has recorded while powered, in hours.",
            "Total de horas que a própria unidade registrou enquanto esteve ligada.",
        ),
        "drive_health" => (
            "The latest alert reported by the drive. A drive can still have problems when it reports no alert.",
            "Último alerta informado pela unidade. Uma unidade ainda pode apresentar problemas mesmo quando não informa um alerta.",
        ),
        "ssd_endurance_used" => (
            "The manufacturer's estimate of rated SSD endurance consumed. It can exceed 100%.",
            "Estimativa do fabricante sobre a resistência do SSD já consumida. O valor pode passar de 100%.",
        ),
        "available_spare" => (
            "Percentage of the SSD's reserve for replacing physical blocks that can no longer be used.",
            "Percentual da reserva do SSD para substituir blocos físicos que já não podem ser usados.",
        ),
        "reallocated_sectors" => (
            "Sectors the drive has already replaced with spare sectors.",
            "Setores que a unidade já substituiu por setores da reserva.",
        ),
        "pending_sectors" => (
            "Sectors the drive has flagged for another check or replacement.",
            "Setores que a unidade sinalizou para nova verificação ou substituição.",
        ),
        "uncorrectable_sectors" => (
            "Sectors the drive reported it could not read or correct during an internal check.",
            "Setores que a unidade informou não conseguir ler ou corrigir durante uma verificação interna.",
        ),
        "unsafe_shutdowns" => (
            "Times the SSD recorded a loss of power without receiving its normal shutdown notification.",
            "Vezes em que o SSD registrou perda de energia sem receber a notificação normal de desligamento.",
        ),
        "battery_charge" => (
            "The charge remaining at capture time as a percentage of the battery's current full charge.",
            "Carga restante no momento da captura em relação à carga completa atual da bateria.",
        ),
        _ => return "",
    };
    if pt { text.1 } else { text.0 }
}
