use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
    ptr::{null, null_mut},
    time::{Duration, Instant},
};

use chrono::Utc;
use nucelvo_core::{Sample, State};
use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows_sys::Win32::System::Performance::*;

const SOURCE: &str = "Win32 PDH · % Processor Time";
const MAX_BUFFER: usize = 1024 * 1024;
const MAX_LOGICAL: usize = 64;
type Processor = (u16, u16);
type CounterValues = BTreeMap<Processor, (State, Option<f64>)>;

#[derive(Default)]
pub struct LogicalSampler {
    query: Option<Query>,
    known: BTreeSet<Processor>,
    last_failure: Option<(Instant, State)>,
}

impl LogicalSampler {
    pub fn sample(&mut self) -> Vec<Sample> {
        let now = Instant::now();
        let at = Utc::now();
        let emit = |processor: Option<Processor>, state, value| {
            let (id, name, name_pt) = match processor {
                Some((group, index)) => (
                    format!("cpu:logical:{group}:{index}"),
                    format!("CPU {group}:{index}"),
                    None,
                ),
                None => (
                    "cpu:logical:status".into(),
                    "Logical CPU counters".into(),
                    Some("Contadores de CPUs lógicas"),
                ),
            };
            Sample {
                seen: now,
                at,
                id,
                name,
                name_pt,
                source: SOURCE,
                unit: "%",
                state,
                value,
            }
        };
        let reading = match self.last_failure {
            Some((failed_at, state)) if now.duration_since(failed_at) < Duration::from_secs(5) => {
                Err(state)
            }
            _ => {
                let result = self.read();
                match result {
                    Ok(_) => self.last_failure = None,
                    Err(state) if state != State::TemporarilyUnavailable => {
                        self.last_failure = Some((now, state));
                        self.query = None;
                    }
                    Err(_) => {}
                }
                result
            }
        };
        match reading {
            Ok(values) => {
                let present = values.keys().copied().collect::<BTreeSet<_>>();
                let mut samples = values
                    .into_iter()
                    .map(|(proc, (state, value))| emit(Some(proc), state, value))
                    .collect::<Vec<_>>();
                samples.extend(
                    self.known
                        .difference(&present)
                        .map(|proc| emit(Some(*proc), State::Stale, None)),
                );
                self.known = present;
                samples
            }
            Err(State::TemporarilyUnavailable) if self.known.is_empty() => Vec::new(),
            Err(state) if self.known.is_empty() => vec![emit(None, state, None)],
            Err(state) => self
                .known
                .iter()
                .map(|proc| emit(Some(*proc), state, None))
                .collect(),
        }
    }

    fn read(&mut self) -> Result<CounterValues, State> {
        if self.query.is_none() {
            self.query = Some(Query::open()?);
        }
        self.query.as_mut().expect("PDH query is open").read()
    }
}

struct Query {
    handle: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    buffer: Vec<u64>,
    primed: bool,
}

impl Query {
    fn open() -> Result<Self, State> {
        let mut handle = null_mut();
        // SAFETY: null selects local real-time counters and handle is writable.
        check(unsafe { PdhOpenQueryW(null(), 0, &mut handle) })?;
        let mut query = Self {
            handle,
            counter: null_mut(),
            buffer: Vec::new(),
            primed: false,
        };
        let path = r"\Processor Information(*)\% Processor Time"
            .encode_utf16()
            .chain([0])
            .collect::<Vec<_>>();
        // SAFETY: query owns a valid handle, path is NUL-terminated and the result slot is writable.
        check(unsafe {
            PdhAddEnglishCounterW(query.handle, path.as_ptr(), 0, &mut query.counter)
        })?;
        Ok(query)
    }

    fn read(&mut self) -> Result<CounterValues, State> {
        // SAFETY: the query handle is live for this call.
        check(unsafe { PdhCollectQueryData(self.handle) })?;
        if !std::mem::replace(&mut self.primed, true) {
            return Err(State::TemporarilyUnavailable);
        }
        for _ in 0..3 {
            let available = self.buffer.len() * size_of::<u64>();
            let mut bytes = available as u32;
            let mut count = 0;
            let output = if self.buffer.is_empty() {
                null_mut()
            } else {
                self.buffer.as_mut_ptr().cast()
            };
            // SAFETY: the counter is owned by the query. The u64 buffer has the size and
            // alignment required by the Windows x64 counter value records.
            let status = unsafe {
                PdhGetFormattedCounterArrayW(
                    self.counter,
                    PDH_FMT_DOUBLE,
                    &mut bytes,
                    &mut count,
                    output,
                )
            };
            if available > 0
                && matches!(
                    status,
                    PDH_MORE_DATA | PDH_INVALID_ARGUMENT | PDH_INSUFFICIENT_BUFFER
                )
            {
                // PDH only promises a reliable required size for a zero-byte probe.
                self.buffer.clear();
                continue;
            }
            if status == PDH_MORE_DATA {
                let bytes = bytes as usize;
                if bytes <= available || bytes > MAX_BUFFER {
                    return Err(State::Error);
                }
                self.buffer.resize(bytes.div_ceil(size_of::<u64>()), 0);
                continue;
            }
            check(status)?;
            if count == 0 {
                return Err(State::TemporarilyUnavailable);
            }
            let used = bytes as usize;
            let records = count as usize * size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if records > used || used > available {
                return Err(State::Error);
            }
            // SAFETY: success initialized the reported contiguous record array; its size
            // was checked against the allocation before slicing it.
            let entries = unsafe { std::slice::from_raw_parts(output, count as usize) };
            let mut values = BTreeMap::new();
            for entry in entries {
                let Some(processor) = processor_name(&self.buffer, used, entry.szName) else {
                    continue;
                };
                let (state, value) = match entry.FmtValue.CStatus {
                    PDH_CSTATUS_VALID_DATA | PDH_CSTATUS_NEW_DATA => {
                        // SAFETY: PDH_FMT_DOUBLE selected the double union member.
                        let value = unsafe { entry.FmtValue.Anonymous.doubleValue };
                        if (0.0..=100.0).contains(&value) {
                            (State::Ok, Some(value))
                        } else {
                            (State::Error, None)
                        }
                    }
                    status => (status_state(status), None),
                };
                values.insert(processor, (state, value));
            }
            return Ok(values.into_iter().take(MAX_LOGICAL).collect());
        }
        Err(State::TemporarilyUnavailable)
    }
}

impl Drop for Query {
    fn drop(&mut self) {
        // SAFETY: this closes the query once and releases its owned counters.
        unsafe {
            PdhCloseQuery(self.handle);
        }
    }
}

fn processor_name(buffer: &[u64], used: usize, name: *const u16) -> Option<Processor> {
    let offset = (name as usize).checked_sub(buffer.as_ptr() as usize)?;
    if used > std::mem::size_of_val(buffer)
        || offset >= used
        || !offset.is_multiple_of(size_of::<u16>())
    {
        return None;
    }
    // SAFETY: the pointer is reconstructed from the aligned, initialized allocation;
    // the slice stops at its byte boundary and is used only while the buffer is borrowed.
    let words = unsafe {
        std::slice::from_raw_parts(
            buffer.as_ptr().cast::<u8>().add(offset).cast::<u16>(),
            ((used - offset) / size_of::<u16>()).min(16),
        )
    };
    let end = words.iter().position(|word| *word == 0)?;
    let name = String::from_utf16(&words[..end]).ok()?;
    let (group, processor) = name.split_once(',')?;
    Some((group.parse().ok()?, processor.parse().ok()?))
}

fn check(status: u32) -> Result<(), State> {
    if status == 0 {
        Ok(())
    } else {
        Err(status_state(status))
    }
}

fn status_state(status: u32) -> State {
    match status {
        PDH_ACCESS_DENIED | ERROR_ACCESS_DENIED => State::PermissionDenied,
        PDH_CSTATUS_NO_OBJECT | PDH_CSTATUS_NO_COUNTER => State::Unsupported,
        PDH_NO_DATA
        | PDH_INVALID_DATA
        | PDH_CSTATUS_INVALID_DATA
        | PDH_CSTATUS_ITEM_NOT_VALIDATED
        | PDH_CSTATUS_NO_INSTANCE
        | PDH_CALC_NEGATIVE_DENOMINATOR
        | PDH_CALC_NEGATIVE_TIMEBASE
        | PDH_CALC_NEGATIVE_VALUE => State::TemporarilyUnavailable,
        _ => State::Error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_require_a_terminated_group_and_processor_within_the_used_buffer() {
        let bytes = [b'2', 0, b',', 0, b'7', 0, 0, 0];
        let buffer = [u64::from_le_bytes(bytes)];
        let name = buffer.as_ptr().cast::<u16>();
        assert_eq!(processor_name(&buffer, 8, name), Some((2, 7)));
        assert_eq!(processor_name(&buffer, 6, name), None);
        assert_eq!(processor_name(&buffer, 8, std::ptr::null()), None);
        assert_eq!(processor_name(&buffer, 9, name), None);
        assert_eq!(status_state(PDH_NO_DATA), State::TemporarilyUnavailable);
        assert_eq!(status_state(PDH_ACCESS_DENIED), State::PermissionDenied);
    }
}
