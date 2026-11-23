use std::{fs, io, path::Path};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en-US")]
    English,
    #[serde(rename = "pt-BR")]
    Portuguese,
}

#[derive(Deserialize, Serialize)]
struct Preferences {
    schema_version: u32,
    language: Language,
}

const SCHEMA_VERSION: u32 = 1;

pub fn load() -> Language {
    ProjectDirs::from("com", "INformigados", "nucelvo")
        .map(|dirs| read(dirs.config_dir()))
        .unwrap_or_default()
}

pub fn save(language: Language) -> io::Result<()> {
    let dirs = ProjectDirs::from("com", "INformigados", "nucelvo")
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    write(dirs.config_dir(), language)
}

fn read(directory: &Path) -> Language {
    fs::read(directory.join("preferences.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
        .filter(|preferences| preferences.schema_version == SCHEMA_VERSION)
        .map(|preferences| preferences.language)
        .unwrap_or_default()
}

fn write(directory: &Path, language: Language) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(
        file.as_file_mut(),
        &Preferences {
            schema_version: SCHEMA_VERSION,
            language,
        },
    )?;
    file.as_file().sync_all()?;
    file.persist(directory.join("preferences.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_survives_restart_and_invalid_preferences_fall_back() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        assert_eq!(read(path), Language::English);
        write(path, Language::Portuguese).unwrap();
        assert_eq!(read(path), Language::Portuguese);
        write(path, Language::English).unwrap();
        assert_eq!(read(path), Language::English);
        for invalid in [
            r#"{"schema_version":1,"language":"other"}"#,
            r#"{"schema_version":2,"language":"pt-BR"}"#,
            "{",
        ] {
            fs::write(path.join("preferences.json"), invalid).unwrap();
            assert_eq!(read(path), Language::English);
        }
    }
}
