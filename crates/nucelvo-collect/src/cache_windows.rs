use std::mem::{offset_of, size_of};
use std::ptr;
use std::slice;

use nucelvo_core::{Category, Field};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError},
    System::SystemInformation::{
        CACHE_RELATIONSHIP, CacheData as CACHE_DATA, CacheInstruction as CACHE_INSTRUCTION,
        CacheUnified as CACHE_UNIFIED, GetLogicalProcessorInformationEx, RelationCache,
        SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    },
};

const SOURCE: &str = "Win32 · processor cache topology";
const CACHE_OFFSET: usize = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous);
const LEVEL: usize = CACHE_OFFSET + offset_of!(CACHE_RELATIONSHIP, Level);
const SIZE: usize = CACHE_OFFSET + offset_of!(CACHE_RELATIONSHIP, CacheSize);
const KIND: usize = CACHE_OFFSET + offset_of!(CACHE_RELATIONSHIP, Type);
const PREFIX_BYTES: usize = KIND + size_of::<i32>();

pub(super) fn fields() -> Vec<Field> {
    let mut length = 0;
    // SAFETY: With a null result buffer, Windows writes the required byte length.
    if unsafe { GetLogicalProcessorInformationEx(RelationCache, ptr::null_mut(), &mut length) } != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
    {
        return Vec::new();
    }
    // Retry if the topology changes between the size query and collection.
    for _ in 0..2 {
        if !(PREFIX_BYTES as u32..=1_048_576).contains(&length) {
            break;
        }
        // u64 storage provides alignment for the Windows x64 structures.
        let mut buffer = vec![0_u64; (length as usize).div_ceil(size_of::<u64>())];
        // SAFETY: The aligned writable allocation holds at least length bytes.
        if unsafe {
            GetLogicalProcessorInformationEx(RelationCache, buffer.as_mut_ptr().cast(), &mut length)
        } != 0
        {
            if length as usize > buffer.len() * size_of::<u64>() {
                break;
            }
            // SAFETY: The successful call filled length bytes within initialized storage.
            let bytes = unsafe { slice::from_raw_parts(buffer.as_ptr().cast(), length as usize) };
            return cache_fields(bytes).unwrap_or_default();
        }
        if unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER {
            break;
        }
    }
    Vec::new()
}

fn cache_fields(mut bytes: &[u8]) -> Option<Vec<Field>> {
    let mut totals = [0_u64; 5];
    while !bytes.is_empty() {
        let header = bytes.first_chunk::<8>()?;
        let relationship = i32::from_le_bytes(header[..4].try_into().ok()?);
        let length = u32::from_le_bytes(header[4..].try_into().ok()?) as usize;
        if length < PREFIX_BYTES || relationship != RelationCache {
            return None;
        }
        let (entry, rest) = bytes.split_at_checked(length)?;
        let capacity = u32::from_le_bytes(entry[SIZE..SIZE + 4].try_into().ok()?);
        let kind = i32::from_le_bytes(entry[KIND..KIND + 4].try_into().ok()?);
        // The API returns one record per cache, including those shared by several logical CPUs.
        let slot = match (entry[LEVEL], kind) {
            (1, CACHE_DATA) => Some(0),
            (1, CACHE_INSTRUCTION) => Some(1),
            (1, CACHE_UNIFIED) => Some(2),
            (2, CACHE_DATA | CACHE_INSTRUCTION | CACHE_UNIFIED) => Some(3),
            (3, CACHE_DATA | CACHE_INSTRUCTION | CACHE_UNIFIED) => Some(4),
            _ => None,
        };
        if let Some(slot) = slot {
            totals[slot] += u64::from(capacity);
        }
        bytes = rest;
    }
    Some(
        [
            (
                "l1_data_cache",
                ("Total L1 data cache", "Total de cache L1 de dados"),
            ),
            (
                "l1_instruction_cache",
                (
                    "Total L1 instruction cache",
                    "Total de cache L1 de instruções",
                ),
            ),
            (
                "l1_unified_cache",
                ("Total unified L1 cache", "Total de cache L1 unificado"),
            ),
            ("l2_cache", ("Total L2 cache", "Total de cache L2")),
            ("l3_cache", ("Total L3 cache", "Total de cache L3")),
        ]
        .into_iter()
        .zip(totals)
        .filter(|&(_, total)| total > 0)
        .map(|((key, label), total)| {
            Field::text(Category::Cpu, 0, key, label, None, SOURCE).number(Some(total), "bytes")
        })
        .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use nucelvo_core::Value;

    #[test]
    fn totals_distinct_caches_and_rejects_truncated_topology() {
        let mut topology = Vec::new();
        for (level, kind, size) in [
            (1, CACHE_DATA, 32768_u32),
            (1, CACHE_DATA, 32768),
            (1, CACHE_INSTRUCTION, 32768),
            (3, CACHE_UNIFIED, 12 * 1024 * 1024),
        ] {
            let mut entry = [0; PREFIX_BYTES];
            entry[..4].copy_from_slice(&RelationCache.to_le_bytes());
            entry[4..8].copy_from_slice(&(PREFIX_BYTES as u32).to_le_bytes());
            entry[LEVEL] = level;
            entry[SIZE..SIZE + 4].copy_from_slice(&size.to_le_bytes());
            entry[KIND..KIND + 4].copy_from_slice(&kind.to_le_bytes());
            topology.extend(entry);
        }
        let caches = cache_fields(&topology).unwrap();
        assert_eq!(caches.len(), 3);
        assert!(matches!(caches[0].value, Some(Value::Number(65536))));
        assert_eq!(caches[1].key, "l1_instruction_cache");
        assert!(matches!(caches[2].value, Some(Value::Number(bytes)) if bytes == 12 * 1024 * 1024));
        assert!(cache_fields(&topology[..topology.len() - 1]).is_none());
    }
}
