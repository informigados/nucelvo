use nucelvo_core::{Category, Field};

pub fn fields() -> Vec<Field> {
    #[cfg(windows)]
    return windows::fields();
    #[cfg(target_os = "linux")]
    return linux::fields(std::path::Path::new("/sys/devices/system/edac/mc"));
}

fn diagnostic(
    key: &'static str,
    labels: (&'static str, &'static str),
    source: &'static str,
) -> Field {
    Field::text(Category::Memory, 10_000, key, labels, None, source)
}

#[cfg(windows)]
mod windows {
    use super::*;
    use chrono::DateTime;
    use nucelvo_core::State;
    use std::mem::size_of_val;
    use windows_sys::{
        Win32::{
            Foundation::{ERROR_ACCESS_DENIED, ERROR_NO_MORE_ITEMS, ERROR_TIMEOUT, GetLastError},
            System::EventLog::{
                EVT_HANDLE, EVT_VARIANT, EvtClose, EvtCreateRenderContext, EvtNext, EvtQuery,
                EvtQueryChannelPath, EvtQueryReverseDirection, EvtRender, EvtRenderContextValues,
                EvtRenderEventValues, EvtVarTypeFileTime, EvtVarTypeUInt16,
            },
        },
        core::w,
    };

    const SOURCE: &str = "Windows · Memory Diagnostics event log";
    const QUERY: windows_sys::core::PCWSTR = w!(
        "*[System[Provider[@Name='Microsoft-Windows-MemoryDiagnostics-Results'] and (EventID=1101 or EventID=1102 or EventID=1103 or EventID=1104 or EventID=1201 or EventID=1202)]]"
    );

    struct Handle(EVT_HANDLE);

    impl Handle {
        fn new(handle: EVT_HANDLE) -> Result<Self, State> {
            if handle == 0 {
                Err(last_error())
            } else {
                Ok(Self(handle))
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: This handle was returned by the event API and is owned by this wrapper.
            unsafe { EvtClose(self.0) };
        }
    }

    fn last_error() -> State {
        // SAFETY: Reads the calling thread's last error without dereferencing a pointer.
        match unsafe { GetLastError() } {
            ERROR_ACCESS_DENIED => State::PermissionDenied,
            ERROR_TIMEOUT => State::TemporarilyUnavailable,
            _ => State::Error,
        }
    }

    fn outcome(id: u16) -> Option<&'static str> {
        match id {
            1101 | 1201 => Some("no_errors_reported"),
            1102 | 1202 => Some("errors_reported"),
            1103 => Some("canceled"),
            1104 => Some("incomplete"),
            _ => None,
        }
    }

    fn latest() -> Result<Option<(u16, Option<String>)>, State> {
        // SAFETY: Static, NUL-terminated UTF-16 paths select a local, read-only query.
        let query = Handle::new(unsafe {
            EvtQuery(
                0,
                w!("System"),
                QUERY,
                EvtQueryChannelPath | EvtQueryReverseDirection,
            )
        })?;
        let mut event = 0;
        let mut returned = 0;
        // SAFETY: Both output slots are writable and the query remains open.
        if unsafe { EvtNext(query.0, 1, &mut event, 450, 0, &mut returned) } == 0 {
            // SAFETY: Reads the result of the failed native call on this thread.
            return if unsafe { GetLastError() } == ERROR_NO_MORE_ITEMS {
                Ok(None)
            } else {
                Err(last_error())
            };
        }
        let event = Handle::new(event)?;
        if returned != 1 {
            return Err(State::Error);
        }
        let paths = [
            w!("Event/System/EventID"),
            w!("Event/System/TimeCreated/@SystemTime"),
        ];
        // SAFETY: The array holds two pointers to static NUL-terminated paths.
        let context = Handle::new(unsafe {
            EvtCreateRenderContext(paths.len() as u32, paths.as_ptr(), EvtRenderContextValues)
        })?;
        let mut values = [EVT_VARIANT::default(); 2];
        let mut bytes = 0;
        let mut count = 0;
        // SAFETY: The aligned buffer holds the two requested scalar event fields.
        if unsafe {
            EvtRender(
                context.0,
                event.0,
                EvtRenderEventValues,
                size_of_val(&values) as u32,
                values.as_mut_ptr().cast(),
                &mut bytes,
                &mut count,
            )
        } == 0
        {
            return Err(last_error());
        }
        if count != 2 || values[0].Type != EvtVarTypeUInt16 as u32 {
            return Err(State::Error);
        }
        // SAFETY: The variant types are checked before reading their union fields.
        let id = unsafe { values[0].Anonymous.UInt16Val };
        let timestamp = (values[1].Type == EvtVarTypeFileTime as u32)
            .then(|| unsafe { values[1].Anonymous.FileTimeVal })
            .and_then(|time| i64::try_from(time / 10).ok())
            .and_then(|micros| micros.checked_sub(11_644_473_600_000_000))
            .and_then(DateTime::from_timestamp_micros)
            .map(|time| time.format("%Y-%m-%d %H:%M UTC").to_string());
        Ok(Some((id, timestamp)))
    }

    pub(super) fn fields() -> Vec<Field> {
        let mut status = diagnostic(
            "memory_diagnostic_status",
            (
                "Latest Windows memory test",
                "Último teste de memória do Windows",
            ),
            SOURCE,
        );
        let mut fields = Vec::with_capacity(2);
        match latest() {
            Ok(event) => {
                let outcome = event
                    .as_ref()
                    .map_or(Some("no_record"), |(id, _)| outcome(*id));
                if let Some(value) = outcome {
                    status.value = Some(nucelvo_core::Value::Text(value.into()));
                    status.state = State::Ok;
                } else {
                    status.state = State::Error;
                }
                if let Some((_, Some(timestamp))) = event {
                    fields.push(Field::text(
                        Category::Memory,
                        10_000,
                        "memory_diagnostic_date",
                        ("Test recorded at", "Teste registrado em"),
                        Some(timestamp),
                        SOURCE,
                    ));
                }
            }
            Err(state) => status.state = state,
        }
        fields.insert(0, status);
        fields
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn distinguishes_completed_error_results_from_incomplete_tests() {
            assert_eq!(outcome(1201), Some("no_errors_reported"));
            assert_eq!(outcome(1102), Some("errors_reported"));
            assert_eq!(outcome(1202), Some("errors_reported"));
            assert_eq!(outcome(1103), Some("canceled"));
            assert_eq!(outcome(1104), Some("incomplete"));
            assert_eq!(outcome(2001), None);
        }
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use super::*;
    use nucelvo_core::Reading;
    use std::{fs, io, path::Path};

    const SOURCE: &str = "Linux · EDAC memory controllers";

    pub(super) fn fields(root: &Path) -> Vec<Field> {
        let controllers = fs::read_dir(root).and_then(|entries| {
            let mut controllers = Vec::new();
            for entry in entries {
                let entry = entry?;
                if entry
                    .file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix("mc"))
                    .is_some_and(|index| index.parse::<u32>().is_ok())
                {
                    controllers.push(entry.path());
                    if controllers.len() > 64 {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                }
            }
            if controllers.is_empty() {
                return Err(io::ErrorKind::Unsupported.into());
            }
            Ok(controllers)
        });
        [
            (
                "ce_count",
                "memory_diagnostic_corrected",
                ("Corrected memory errors", "Erros de memória corrigidos"),
            ),
            (
                "ue_count",
                "memory_diagnostic_uncorrected",
                (
                    "Uncorrected memory errors",
                    "Erros de memória não corrigidos",
                ),
            ),
        ]
        .into_iter()
        .map(|(file, key, labels)| {
            let count = match &controllers {
                Ok(controllers) => controllers.iter().try_fold(0_u64, |total, controller| {
                    let value = fs::read_to_string(controller.join(file))?
                        .trim()
                        .parse::<u64>()
                        .map_err(|_| io::ErrorKind::InvalidData)?;
                    total
                        .checked_add(value)
                        .ok_or_else(|| io::ErrorKind::InvalidData.into())
                }),
                Err(error) => Err(io::Error::from(error.kind())),
            };
            let reading = Reading::from_result(count, SOURCE);
            let mut field = diagnostic(key, labels, SOURCE).number(reading.value, "");
            field.state = reading.state;
            field
        })
        .collect()
    }

    #[test]
    fn zero_is_a_reported_counter_and_a_missing_controller_value_is_not_zero() {
        use nucelvo_core::{State, Value};

        let root = tempfile::tempdir().unwrap();
        assert_eq!(fields(root.path())[0].state, State::Unsupported);
        for (name, corrected) in [("mc0", "0"), ("mc1", "8")] {
            let controller = root.path().join(name);
            fs::create_dir(&controller).unwrap();
            fs::write(controller.join("ce_count"), corrected).unwrap();
            fs::write(controller.join("ue_count"), "0").unwrap();
        }
        let counters = fields(root.path());
        assert!(matches!(counters[0].value, Some(Value::Number(8))));
        assert!(matches!(counters[1].value, Some(Value::Number(0))));
        fs::remove_file(root.path().join("mc1/ue_count")).unwrap();
        assert!(fields(root.path())[1].value.is_none());
    }
}
