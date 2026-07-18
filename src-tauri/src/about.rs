//! App identity ("About") and GitHub-release update checking.
//!
//! The version is read from the crate's build metadata (`CARGO_PKG_VERSION`)
//! rather than hardcoded, so it always matches the shipped binary. The update
//! check queries the public GitHub "latest release" endpoint and reuses the
//! configured outbound proxy via [`crate::providers::build_client`].

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::models::ProxySettings;

const PACKAGE_JSON: &str = include_str!("../../package.json");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PackageMetadata {
    product_name: String,
    repository: PackageRepository,
}

#[derive(Debug, Deserialize)]
struct PackageRepository {
    url: String,
}

/// Static app identity shown in the Settings "About" card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    /// The package author, sourced from Cargo.toml.
    pub author: String,
    /// The running build's version, sourced from `CARGO_PKG_VERSION`.
    pub version: String,
    pub github_url: String,
}

/// Result of comparing the running version against the latest GitHub release.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    /// The release page to open in the browser; falls back to the repo's
    /// `/releases` listing when the API omits an `html_url`.
    pub release_url: String,
    pub release_name: Option<String>,
    pub published_at: Option<String>,
    /// RFC 3339 timestamp of when this check ran.
    pub checked_at: String,
}

/// Subset of the GitHub release payload we consume. Extra fields (including
/// `body`) are ignored.
#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
}

fn package_metadata() -> PackageMetadata {
    serde_json::from_str(PACKAGE_JSON).expect("package.json must contain valid app metadata")
}

pub fn product_name() -> String {
    package_metadata().product_name
}

fn normalize_repository_url(url: &str) -> String {
    url.trim()
        .trim_start_matches("git+")
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .to_string()
}

fn github_latest_release_api(repository_url: &str) -> String {
    let normalized = normalize_repository_url(repository_url);
    let repository = normalized
        .strip_prefix("https://github.com/")
        .expect("package.json repository must be a GitHub HTTPS URL");
    format!("https://api.github.com/repos/{repository}/releases/latest")
}

/// Build the current app identity from package.json and Cargo build metadata.
pub fn app_info() -> AppInfo {
    let metadata = package_metadata();
    AppInfo {
        name: metadata.product_name,
        author: env!("CARGO_PKG_AUTHORS").to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        github_url: normalize_repository_url(&metadata.repository.url),
    }
}

/// Split a version string into numeric components for comparison.
///
/// Tolerates a leading `v`/`V` and non-numeric suffixes on any component
/// (e.g. `1.2.0-rc1` -> `[1, 2, 0]`); non-numeric leads parse as 0.
fn parse_version(raw: &str) -> Vec<u64> {
    let trimmed = raw.trim();
    let trimmed = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);
    trimmed
        .split('.')
        .map(|part| {
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().unwrap_or(0)
        })
        .collect()
}

/// Compare two version strings numerically, component by component, treating a
/// missing trailing component as 0 (so `1.0` == `1.0.0`).
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let va = parse_version(a);
    let vb = parse_version(b);
    let len = va.len().max(vb.len());
    for i in 0..len {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            Ordering::Equal => continue,
            other => return other,
        }
    }
    Ordering::Equal
}

/// Whether `latest` is strictly newer than `current`.
pub fn is_update_available(current: &str, latest: &str) -> bool {
    compare_versions(latest, current) == Ordering::Greater
}

/// Query GitHub for the latest release and compare it to the running version.
///
/// Honors the configured proxy. A missing release, non-2xx status, or network
/// failure surfaces as an [`AppError::Request`] so the frontend can show a
/// clear error instead of hanging.
pub async fn check_for_update(proxy: &ProxySettings) -> AppResult<UpdateCheckResult> {
    let info = app_info();
    let current = info.version;
    let latest_release_api = github_latest_release_api(&info.github_url);
    let client = crate::providers::build_client(proxy)?;
    // GitHub rejects requests without a User-Agent; identify the app + version.
    let user_agent = format!("{}/{current}", env!("CARGO_PKG_NAME"));

    let resp = client
        .get(latest_release_api)
        .header("User-Agent", user_agent)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::Request(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(AppError::Request(format!(
            "GitHub API returned status {}",
            resp.status()
        )));
    }

    let release: GithubRelease = resp
        .json()
        .await
        .map_err(|e| AppError::Request(e.to_string()))?;

    let latest = release.tag_name.trim().to_string();
    let update_available = is_update_available(&current, &latest);
    let release_url = if release.html_url.trim().is_empty() {
        format!("{}/releases", info.github_url)
    } else {
        release.html_url
    };

    Ok(UpdateCheckResult {
        current_version: current,
        latest_version: latest,
        update_available,
        release_url,
        release_name: release.name,
        published_at: release.published_at,
        checked_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_comes_from_package_and_cargo_metadata() {
        let info = app_info();
        let package = package_metadata();

        assert_eq!(info.name, package.product_name);
        assert_eq!(info.author, env!("CARGO_PKG_AUTHORS"));
        assert_eq!(info.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            info.github_url,
            normalize_repository_url(&package.repository.url)
        );
    }

    #[test]
    fn github_api_is_derived_from_repository_metadata() {
        assert_eq!(
            github_latest_release_api("https://github.com/example/project.git"),
            "https://api.github.com/repos/example/project/releases/latest"
        );
    }

    #[test]
    fn newer_patch_is_greater() {
        assert_eq!(compare_versions("0.1.5", "0.1.4"), Ordering::Greater);
        assert!(is_update_available("0.1.4", "0.1.5"));
    }

    #[test]
    fn v_prefix_equals_plain() {
        assert_eq!(compare_versions("v1.0.0", "1.0.0"), Ordering::Equal);
        assert!(!is_update_available("v1.0.0", "1.0.0"));
        assert!(!is_update_available("1.0.0", "v1.0.0"));
    }

    #[test]
    fn compares_numerically_not_lexically() {
        assert_eq!(compare_versions("0.10.0", "0.9.9"), Ordering::Greater);
        assert!(is_update_available("0.9.9", "0.10.0"));
    }

    #[test]
    fn no_update_when_current_is_newer() {
        assert!(!is_update_available("0.2.0", "0.1.9"));
        assert_eq!(compare_versions("0.1.9", "0.2.0"), Ordering::Less);
    }

    #[test]
    fn missing_trailing_component_is_zero() {
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("v1.2", "1.2.0"), Ordering::Equal);
    }

    #[test]
    fn tolerates_prerelease_suffix() {
        assert_eq!(compare_versions("1.2.0-rc1", "1.2.0"), Ordering::Equal);
    }
}
