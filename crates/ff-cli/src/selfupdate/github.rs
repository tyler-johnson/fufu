//! GitHub Releases API client for self-update: fufu's own latest release,
//! and a declared extension's when its `releases` recipe is a github.com
//! page.

use serde::Deserialize;

/// fufu's own repository, as the API names it.
pub const FUFU_REPO: &str = "tyler-johnson/fufu";

/// Only the tag is read: fufu no longer downloads assets, it compares
/// versions and hands the install script the rest.
#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(180)))
        .http_status_as_error(false)
        .build()
        .into()
}

pub fn get(agent: &ureq::Agent, url: &str) -> ff_core::Result<ureq::http::Response<ureq::Body>> {
    let mut req = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "ff");

    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && !token.is_empty()
    {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    req.call()
        .map_err(|err| ff_core::Error::msg(format!("cannot reach GitHub: {err}")))
}

/// fufu's own latest release.
pub fn fetch_latest(agent: &ureq::Agent, api_base: &str) -> ff_core::Result<Release> {
    fetch_latest_of(agent, api_base, FUFU_REPO)
}

/// The latest release of `repo`, an `owner/name` as the API takes it:
/// fufu's own, or the one a declared extension's `releases` page names.
pub fn fetch_latest_of(
    agent: &ureq::Agent,
    api_base: &str,
    repo: &str,
) -> ff_core::Result<Release> {
    let url = format!("{api_base}/repos/{repo}/releases/latest");
    let mut resp = get(agent, &url)?;

    let status = resp.status().as_u16();
    if status == 403
        && let Some(remaining) = resp
            .headers()
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
        && remaining == "0"
    {
        return Err(ff_core::Error::msg(
            "GitHub API rate limit hit — try again later, or set GITHUB_TOKEN",
        ));
    }

    if !(200..300).contains(&status) {
        return Err(ff_core::Error::msg(format!(
            "GitHub API error: HTTP {status}"
        )));
    }

    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|err| ff_core::Error::msg(format!("cannot read GitHub API response: {err}")))?;

    serde_json::from_str(&body).map_err(|_| ff_core::Error::msg("unexpected GitHub API response"))
}

/// The `owner/name` a github.com URL names, or `None` for any other host.
///
/// This is how the passive lane finds an extension's repository: its
/// manifest's `releases` recipe is the page a person goes to, and on
/// github.com the page names the repository the API answers for, so the
/// manifest carries one URL and not a second field spelling the same
/// thing. A page anywhere else names nothing fufu knows how to ask, and
/// the extension gets no release check rather than a guess.
pub fn repo_from_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let rest = rest
        .strip_prefix("github.com/")
        .or_else(|| rest.strip_prefix("www.github.com/"))?;
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|part| !part.is_empty())?;
    let name = parts
        .next()
        .map(|name| name.strip_suffix(".git").unwrap_or(name))
        .filter(|part| !part.is_empty())?;
    Some(format!("{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A github.com page names its repository whatever comes after the
    /// two path segments: the latest page, the list, one tag, or nothing.
    #[test]
    fn a_github_page_names_its_repository() {
        for url in [
            "https://github.com/tyler-johnson/tower/releases/latest",
            "https://github.com/tyler-johnson/tower/releases",
            "https://github.com/tyler-johnson/tower/releases/tag/v0.2.0",
            "https://github.com/tyler-johnson/tower",
            "https://github.com/tyler-johnson/tower/",
            "https://github.com/tyler-johnson/tower.git",
            "https://www.github.com/tyler-johnson/tower/releases/latest",
            "http://github.com/tyler-johnson/tower/releases/latest",
        ] {
            assert_eq!(
                repo_from_url(url).as_deref(),
                Some("tyler-johnson/tower"),
                "{url}"
            );
        }
    }

    /// Any other host, and a github.com URL short of a repository, names
    /// nothing: the extension gets no release check.
    #[test]
    fn another_host_names_nothing() {
        for url in [
            "https://gitlab.com/tyler-johnson/tower/-/releases",
            "https://example.com/tower/releases",
            "https://raw.githubusercontent.com/tyler-johnson/tower/main/install.sh",
            "https://github.com/",
            "https://github.com/tyler-johnson",
            "https://github.com/tyler-johnson/",
            "https://github.com//tower",
            "github.com/tyler-johnson/tower",
            "",
        ] {
            assert_eq!(repo_from_url(url), None, "{url}");
        }
    }
}
