use std::{io::Read, thread, time::Duration};

use semver::Version;
use serde::Deserialize;
use slint::{ComponentHandle, SharedString};

use crate::MainWindow;

const REPOSITORY: &str = "https://github.com/informigados/nucelvo";
const RELEASES_API: &str = "https://api.github.com/repos/informigados/nucelvo/releases?per_page=20";
const RESPONSE_LIMIT: u64 = 512 * 1024;
const CURRENT: &str = env!("CARGO_PKG_VERSION");

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
}

fn newer_release(releases: &[Release], current: &Version) -> (bool, Option<Version>) {
    // Preview builds accept GitHub previews. Stable builds stay on the stable channel.
    let include_preview = current.major == 0 || !current.pre.is_empty();
    let newest = releases
        .iter()
        .filter(|release| !release.draft && (include_preview || !release.prerelease))
        .filter_map(|release| Version::parse(release.tag_name.strip_prefix('v')?).ok())
        .filter(|version| version.build.is_empty() && (include_preview || version.pre.is_empty()))
        .max();
    (
        newest.is_some(),
        newest.filter(|version| version.cmp_precedence(current).is_gt()),
    )
}

fn check() -> Result<Option<Version>, ()> {
    let response = reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("Nucelvo/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| ())?
        .get(RELEASES_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let mut data = Vec::new();
    response
        .take(RESPONSE_LIMIT + 1)
        .read_to_end(&mut data)
        .map_err(|_| ())?;
    if data.len() as u64 > RESPONSE_LIMIT {
        return Err(());
    }
    let releases: Vec<Release> = serde_json::from_slice(&data).map_err(|_| ())?;
    let current = Version::parse(CURRENT).map_err(|_| ())?;
    let (found, newer) = newer_release(&releases, &current);
    found.then_some(newer).ok_or(())
}

pub fn bind(window: &MainWindow) {
    window.set_app_version(CURRENT.into());
    window.set_copyright_year(
        chrono::Datelike::year(&chrono::Local::now())
            .to_string()
            .into(),
    );
    let weak = window.as_weak();
    window.on_check_updates(move || {
        let Some(window) = weak.upgrade() else { return };
        if window.get_update_state() == 1 {
            return;
        }
        window.set_update_state(1);
        window.set_update_version(SharedString::default());
        let weak = window.as_weak();
        if thread::Builder::new()
            .name("check-updates".into())
            .spawn(move || {
                let outcome = check();
                let _ = weak.upgrade_in_event_loop(move |window| match outcome {
                    Ok(Some(version)) => {
                        window.set_update_version(version.to_string().into());
                        window.set_update_state(3);
                    }
                    Ok(None) => window.set_update_state(2),
                    Err(()) => window.set_update_state(4),
                });
            })
            .is_err()
        {
            window.set_update_state(4);
        }
    });

    let weak = window.as_weak();
    window.on_about_link(move |link| {
        let Some(window) = weak.upgrade() else { return };
        let url = match link {
            0 => "https://github.com/informigados/".into(),
            1 => "https://github.com/alexbritodev".into(),
            2 => "https://informigados.com.br/".into(),
            3 => REPOSITORY.into(),
            4 => match Version::parse(window.get_update_version().as_str()) {
                Ok(version) => format!("{REPOSITORY}/releases/tag/v{version}"),
                Err(_) => return,
            },
            _ => return,
        };
        window.set_about_link_failed(false);
        let weak = window.as_weak();
        if thread::Builder::new()
            .name("open-about-link".into())
            .spawn(move || {
                if webbrowser::open(&url).is_err() {
                    let _ = weak.upgrade_in_event_loop(|window| window.set_about_link_failed(true));
                }
            })
            .is_err()
        {
            window.set_about_link_failed(true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_comparison_respects_versions_drafts_and_release_channel() {
        let releases = serde_json::from_str::<Vec<Release>>(
            r#"[
            {"tag_name":"v2.0.0","draft":true,"prerelease":false},
            {"tag_name":"v0.9.0","draft":false,"prerelease":true},
            {"tag_name":"v0.10.0","draft":false,"prerelease":true},
            {"tag_name":"nightly","draft":false,"prerelease":false},
            {"tag_name":"v1.1.0-rc.1","draft":false,"prerelease":true},
            {"tag_name":"v1.0.0","draft":false,"prerelease":false}
        ]"#,
        )
        .unwrap();
        let version = |text| Version::parse(text).unwrap();
        assert_eq!(
            newer_release(&releases[1..3], &version("0.4.0")),
            (true, Some(version("0.10.0")))
        );
        assert_eq!(newer_release(&releases, &version("1.0.0")), (true, None));
        assert_eq!(
            newer_release(&releases, &version("1.0.0-beta.1")),
            (true, Some(version("1.1.0-rc.1")))
        );
        assert_eq!(
            newer_release(&releases[..1], &version("0.4.0")),
            (false, None)
        );
    }
}
