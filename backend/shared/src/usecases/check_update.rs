use std::time::Duration;

use anyhow::Context;
use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct CheckUpdateResponse {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
    /// Indicates if the update can be automatically installed (i.e., not a major version bump).
    pub auto_installable: bool,
}

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

pub async fn check_update(current_version: String) -> anyhow::Result<CheckUpdateResponse> {
    let current_version = Version::parse(current_version.trim_start_matches("v"))
        .context("Failed to parse current version")?;
    // Get latest release from GitHub API
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.github.com/repos/tachibana-shin/rakuyomi/releases/latest")
        .header("User-Agent", "rakuyomi")
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .context("Failed to fetch latest release")?
        .error_for_status()
        .context("Failed to fetch latest release")?;

    let latest_release = response
        .json::<GitHubRelease>()
        .await
        .context("Failed to parse GitHub API response")?;

    let latest_version = Version::parse(latest_release.tag_name.trim_start_matches('v'))
        .context("Failed to parse latest version")?;
    let available = latest_version > current_version;
    let auto_installable = available && latest_version.major == current_version.major;

    Ok(CheckUpdateResponse {
        available,
        current_version: current_version.to_string(),
        latest_version: latest_version.to_string(),
        release_url: latest_release.html_url,
        auto_installable,
    })
}
