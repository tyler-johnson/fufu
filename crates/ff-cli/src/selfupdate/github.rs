//! GitHub Releases API client for self-update.

use serde::Deserialize;

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

pub fn fetch_latest(agent: &ureq::Agent, api_base: &str) -> ff_core::Result<Release> {
    let url = format!("{api_base}/repos/tyler-johnson/fufu/releases/latest");
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
