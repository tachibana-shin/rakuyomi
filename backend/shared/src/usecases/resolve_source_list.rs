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

/// Last-resort `plugins/vX.Y.Z` branch used when neither the GitHub branches
/// API nor the branches HTML page can be reached. Keep in sync with the
/// latest plugin index branch.
const LNREADER_FALLBACK_BRANCH: &str = "plugins/v3.0.0";

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
/// corresponding raw URL. If the API is unreachable (rate limit, network
/// error) the repository's branches page is scraped instead, which has no API
/// rate limit; if that fails too, a pinned fallback branch is used. Either
/// way a working index URL is preferred over the original repository page.
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

    let branch = latest_lnreader_branch(&owner, &repo)
        .await
        .unwrap_or_else(|| LNREADER_FALLBACK_BRANCH.to_string());
    let resolved = raw_source_list_url(&owner, &repo, &branch);

    RESOLVED_LISTS
        .lock()
        .unwrap()
        .insert(key, (Instant::now(), resolved.clone()));

    resolved
}

/// Builds the raw URL of the LNReader plugin index for the given branch.
fn raw_source_list_url(owner: &str, repo: &str, branch: &str) -> Url {
    format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/.dist/plugins.min.json")
        .parse()
        .expect("hardcoded raw GitHub URL is valid")
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
///
/// The GitHub branches REST API is tried first; when it is unavailable
/// (unauthenticated requests are rate limited per IP at 60/hour), the
/// repository's branches HTML page is scraped instead, which has no such
/// limit.
async fn latest_lnreader_branch(owner: &str, repo: &str) -> Option<String> {
    let client = tls::client_builder().build().ok()?;

    let api_url = format!("https://api.github.com/repos/{owner}/{repo}/branches?per_page=100");
    if let Ok(response) = client.get(&api_url).send().await {
        if let Ok(value) = response.json::<serde_json::Value>().await {
            if let Some(branches) = value.as_array() {
                let names = branches
                    .iter()
                    .filter_map(|branch| branch.get("name")?.as_str())
                    .collect::<Vec<_>>();
                if let Some(branch) = select_latest_branch(&names) {
                    return Some(branch.to_string());
                }
            }
        }
    }

    let html_url = format!("https://github.com/{owner}/{repo}/branches");
    let html = client.get(&html_url).send().await.ok()?.text().await.ok()?;
    let names = find_branch_names_in_html(&html);
    let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
    select_latest_branch(&refs).map(str::to_string)
}

/// Extracts `plugins/vX.Y.Z` branch names from the repository branches page
/// HTML. The names appear both in plain text and in `href` attributes of the
/// branch rows.
fn find_branch_names_in_html(html: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = html;
    while let Some(idx) = rest.find("plugins/v") {
        rest = &rest[idx + "plugins/v".len()..];
        let mut version = String::new();
        for c in rest.chars() {
            if c.is_ascii_digit() || c == '.' {
                version.push(c);
            } else {
                break;
            }
        }
        if version.matches('.').count() == 2 && !version.ends_with('.') {
            names.push(format!("plugins/v{version}"));
        }
    }
    names
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
    fn test_find_branch_names_in_html() {
        let html = concat!(
            "<a href=\"/lnreader/lnreader-plugins/tree/plugins%2Fv2.0.0\">plugins/v2.0.0</a>",
            "<a href=\"/lnreader/lnreader-plugins/tree/plugins%2Fv3.0.0\">plugins/v3.0.0</a>",
            "<a href=\"/lnreader/lnreader-plugins/tree/master\">master</a>",
            // Not a branch row: a version pinned in documentation text.
            "see plugins/v9.9.9.9 for details"
        );
        assert_eq!(
            find_branch_names_in_html(html),
            vec!["plugins/v2.0.0".to_string(), "plugins/v3.0.0".to_string()]
        );
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
