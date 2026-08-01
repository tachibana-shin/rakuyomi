use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use url::Url;

use crate::{
    settings::{SourceList, SourceListType},
    tls,
};

/// How long a resolved source list URL is remembered before the GitHub API is
/// asked again for a newer version. This keeps us comfortably inside the
/// unauthenticated GitHub API rate limit (60 requests/hour).
const RESOLVE_CACHE_TTL: Duration = Duration::from_secs(30 * 60);

static RESOLVED_LISTS: LazyLock<Mutex<HashMap<String, (Instant, Url)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The identifier under which sources from a source list are grouped and
/// matched when installing.
///
/// For GitHub URLs (`github.com` and `raw.githubusercontent.com`) this is
/// `owner/repo`, which stays stable even when the index moves to a newer
/// version-pinned branch. For every other host the URL's domain is used.
pub fn source_list_key(list: &SourceList) -> String {
    if let Some((owner, repo)) = github_repo_segments(&list.url) {
        return format!("{owner}/{repo}");
    }
    list.url.domain().unwrap_or_default().to_string()
}

/// Resolves a source list to the URL that should actually be fetched.
///
/// The official LNReader plugin index is published to a version-pinned branch
/// (`plugins/vX.Y.Z`) with no stable "latest" URL, so `lnreader` lists are
/// asked the GitHub API for the newest `plugins/v*` branch, returning the
/// corresponding raw URL. Any failure (network error, rate limit, unexpected
/// format) falls back to the original URL, which still points at a valid
/// index.
pub async fn resolve_source_list(list: &SourceList) -> Url {
    let url = &list.url;
    if list.source_type != SourceListType::LnReader {
        return url.clone();
    }

    let Some((owner, repo)) = github_repo_segments(url) else {
        return url.clone();
    };

    let key = format!("{:?}:{url}", list.source_type);
    if let Some((cached_at, cached)) = RESOLVED_LISTS.lock().unwrap().get(&key) {
        if cached_at.elapsed() < RESOLVE_CACHE_TTL {
            return cached.clone();
        }
    }

    let resolved = match latest_lnreader_branch(&owner, &repo).await {
        Some(branch) => format!(
            "https://raw.githubusercontent.com/{owner}/{repo}/{branch}/.dist/plugins.min.json"
        )
        .parse()
        .unwrap_or_else(|_| url.clone()),
        None => url.clone(),
    };

    RESOLVED_LISTS
        .lock()
        .unwrap()
        .insert(key, (Instant::now(), resolved.clone()));

    resolved
}

/// Extracts the `owner` and `repo` from a GitHub URL, either the repository
/// page (`https://github.com/owner/repo`) or a raw file URL
/// (`https://raw.githubusercontent.com/owner/repo/<path>`).
fn github_repo_segments(url: &Url) -> Option<(String, String)> {
    match url.host_str() {
        Some("github.com") | Some("raw.githubusercontent.com") => {}
        _ => return None,
    }
    let mut segments = url.path_segments()?.filter(|segment| !segment.is_empty());
    let owner = segments.next()?.to_string();
    let repo = segments.next()?.to_string();
    Some((owner, repo))
}

/// Fetches the list of branches of the given repository and returns the name
/// of the newest `plugins/v*` branch, if any.
async fn latest_lnreader_branch(owner: &str, repo: &str) -> Option<String> {
    let api_url = format!("https://api.github.com/repos/{owner}/{repo}/branches?per_page=100");
    let client = tls::client_builder().build().ok()?;
    let response = client.get(&api_url).send().await.ok()?;
    let value: serde_json::Value = response.json().await.ok()?;
    let branches = value.as_array()?;
    select_latest_branch(
        &branches
            .iter()
            .filter_map(|branch| branch.get("name")?.as_str())
            .collect::<Vec<_>>(),
    )
    .map(str::to_string)
}

/// Selects the newest `plugins/vX.Y.Z` branch out of a list of branch names.
fn select_latest_branch<'a>(branches: &[&'a str]) -> Option<&'a str> {
    branches
        .iter()
        .filter_map(|name| {
            name.strip_prefix("plugins/v")
                .map(|version| (version, *name))
        })
        .filter_map(|(version, name)| parse_version(version).map(|parsed| (parsed, name)))
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, name)| name)
}

/// Parses a dotted numeric version ("3.0.0", "2.1") into a comparable tuple,
/// ignoring any pre-release suffix ("3.0.0-rc1" parses as 3.0.0).
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split(['-', '+']);
    let mut numbers = parts.next()?.split('.');
    let major = numbers.next()?.parse().ok()?;
    let minor = numbers.next().unwrap_or("0").parse().ok()?;
    let patch = numbers.next().unwrap_or("0").parse().ok()?;
    if numbers.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("3.0.0"), Some((3, 0, 0)));
        assert_eq!(parse_version("2.1"), Some((2, 1, 0)));
        assert_eq!(parse_version("10.2.3-rc1"), Some((10, 2, 3)));
        assert_eq!(parse_version("not-a-version"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn test_select_latest_branch() {
        let branches = [
            "plugins/v2.0.0",
            "master",
            "plugins/v10.0.0",
            "plugins/v3.0.0",
        ];
        assert_eq!(select_latest_branch(&branches), Some("plugins/v10.0.0"));

        let branches = ["master", "beta"];
        assert_eq!(select_latest_branch(&branches), None);
    }

    #[test]
    fn test_source_list_key() {
        let url = Url::parse(
            "https://raw.githubusercontent.com/lnreader/lnreader-plugins/plugins/v3.0.0/.dist/plugins.min.json",
        )
        .unwrap();
        let list = SourceList {
            url,
            source_type: SourceListType::Aidoku,
        };
        assert_eq!(source_list_key(&list), "lnreader/lnreader-plugins");

        let url = Url::parse("https://github.com/lnreader/lnreader-plugins").unwrap();
        let list = SourceList {
            url,
            source_type: SourceListType::LnReader,
        };
        assert_eq!(source_list_key(&list), "lnreader/lnreader-plugins");

        let url = Url::parse("https://tachibana-shin.github.io/aidoku-sources-next/index.min.json")
            .unwrap();
        let list = SourceList {
            url,
            source_type: SourceListType::Aidoku,
        };
        assert_eq!(source_list_key(&list), "tachibana-shin.github.io");
    }
}
