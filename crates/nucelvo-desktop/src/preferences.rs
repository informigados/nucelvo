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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    #[serde(other)]
    System,
}

impl Theme {
    pub fn choice(self) -> i32 {
        match self {
            Self::System => 0,
            Self::Light => 1,
            Self::Dark => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Preferences {
    schema_version: u32,
    pub language: Language,
    #[serde(default)]
    pub theme: Theme,
}

const SCHEMA_VERSION: u32 = 1;

impl Default for Preferences {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            language: Language::default(),
            theme: Theme::default(),
        }
    }
}

pub fn load() -> Preferences {
    ProjectDirs::from("com", "INformigados", "nucelvo")
        .map(|dirs| read(dirs.config_dir()))
        .unwrap_or_default()
}

pub fn save(preferences: Preferences) -> io::Result<()> {
    let dirs = ProjectDirs::from("com", "INformigados", "nucelvo")
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    write(dirs.config_dir(), preferences)
}

fn read(directory: &Path) -> Preferences {
    fs::read(directory.join("preferences.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Preferences>(&bytes).ok())
        .filter(|preferences| preferences.schema_version == SCHEMA_VERSION)
        .unwrap_or_default()
}

fn write(directory: &Path, preferences: Preferences) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(file.as_file_mut(), &preferences)?;
    file.as_file().sync_all()?;
    file.persist(directory.join("preferences.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_and_language_survive_restart_and_legacy_files_follow_the_system() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path();
        assert_eq!(read(path), Preferences::default());
        fs::write(
            path.join("preferences.json"),
            r#"{"schema_version":1,"language":"pt-BR"}"#,
        )
        .unwrap();
        let mut preferences = read(path);
        assert_eq!(preferences.language, Language::Portuguese);
        assert_eq!(preferences.theme, Theme::System);
        fs::write(
            path.join("preferences.json"),
            r#"{"schema_version":1,"language":"pt-BR","theme":"future"}"#,
        )
        .unwrap();
        assert_eq!(read(path), preferences);
        preferences.theme = Theme::Dark;
        write(path, preferences).unwrap();
        assert_eq!(read(path), preferences);
        preferences.language = Language::English;
        write(path, preferences).unwrap();
        assert_eq!(read(path), preferences);
        for invalid in [
            r#"{"schema_version":1,"language":"other"}"#,
            r#"{"schema_version":2,"language":"pt-BR"}"#,
            "{",
        ] {
            fs::write(path.join("preferences.json"), invalid).unwrap();
            assert_eq!(read(path), Preferences::default());
        }
    }
}
