use serde::Serialize;
mod details;
pub use details::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Unsupported,
    PermissionDenied,
    TemporarilyUnavailable,
    Stale,
    Error,
}

impl State {
    pub fn label(self, pt: bool) -> &'static str {
        let labels = match self {
            Self::Ok => ("Available", "Disponível"),
            Self::Unsupported => ("Unavailable", "Indisponível"),
            Self::PermissionDenied => ("Permission denied", "Permissão negada"),
            Self::TemporarilyUnavailable => ("Waiting for data", "Aguardando dados"),
            Self::Stale => ("Out of date", "Desatualizado"),
            Self::Error => ("Could not read", "Falha na leitura"),
        };
        if pt { labels.1 } else { labels.0 }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Reading<T> {
    pub value: Option<T>,
    pub state: State,
    pub source: &'static str,
}

impl<T> Reading<T> {
    pub fn from_result(result: std::io::Result<T>, source: &'static str) -> Self {
        use std::io::ErrorKind;
        match result {
            Ok(value) => Self {
                value: Some(value),
                state: State::Ok,
                source,
            },
            Err(error) => Self {
                value: None,
                state: match error.kind() {
                    ErrorKind::NotFound | ErrorKind::Unsupported => State::Unsupported,
                    ErrorKind::PermissionDenied => State::PermissionDenied,
                    ErrorKind::WouldBlock | ErrorKind::Interrupted => State::TemporarilyUnavailable,
                    _ => State::Error,
                },
                source,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Inventory {
    pub processor: Reading<String>,
    pub logical_processors: Reading<usize>,
    pub memory_bytes: Reading<u64>,
    pub operating_system: Reading<String>,
    pub architecture: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct CpuSample {
    pub device_id: &'static str,
    pub metric_id: &'static str,
    pub unit: &'static str,
    pub sampled_at_unix_ms: u128,
    pub reading: Reading<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn readings_distinguish_missing_data_from_a_measured_zero() {
        let zero = Reading::from_result(Ok(0), "test");
        assert_eq!(zero.value, Some(0));
        assert_eq!(zero.state, State::Ok);

        for (kind, state) in [
            (ErrorKind::NotFound, State::Unsupported),
            (ErrorKind::PermissionDenied, State::PermissionDenied),
            (ErrorKind::WouldBlock, State::TemporarilyUnavailable),
            (ErrorKind::InvalidData, State::Error),
        ] {
            let reading = Reading::<u32>::from_result(Err(Error::from(kind)), "test");
            assert_eq!(reading.value, None);
            assert_eq!(reading.state, state);
            assert_eq!(reading.source, "test");
        }
    }
}
