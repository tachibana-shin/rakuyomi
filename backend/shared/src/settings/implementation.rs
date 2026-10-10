use std::{fs::File, path::Path};

use anyhow::{Context, Result};

use super::schema::{default_oauth_server_url, Settings};

/// Host of the cookie sync bot before it moved to Cloudflare Workers. Saved
/// settings that still point at it are rewritten on load, otherwise installs
/// that already paired keep talking to a host that is about to disappear.
const LEGACY_BOT_URL: &str = "https://rakuyomi.tachibana-shin.deno.net/";

impl Settings {
    pub fn from_file(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| "Couldn't open file")?;
        let mut settings: Settings = serde_json_lenient::from_reader(file)
            .with_context(|| "Couldn't parse file contents")?;

        if settings.concurrent_requests_pages.is_none() {
            settings.concurrent_requests_pages =
                Some(if cfg!(target_arch = "arm") && cfg!(target_os = "linux") {
                    4
                } else {
                    5
                });
        }

        settings.migrate_deprecated_bot_url();

        Ok(settings)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let file = File::create(path)?;

        Ok(serde_json_lenient::to_writer_pretty(file, self)?)
    }

    /// Drops the Deno Deploy URL in favour of the current default. Only the
    /// known legacy value is touched, so a self-hosted URL is left alone.
    pub(crate) fn migrate_deprecated_bot_url(&mut self) {
        if same_origin(&self.oauth_server_url, LEGACY_BOT_URL) {
            self.oauth_server_url = default_oauth_server_url();
        }

        if self
            .cookie_sync_server_url
            .as_deref()
            .is_some_and(|url| same_origin(url, LEGACY_BOT_URL))
        {
            self.cookie_sync_server_url = Some(default_oauth_server_url());
        }
    }
}

/// Compares two URLs ignoring the trailing slash, so both
/// `https://host` and `https://host/` match.
fn same_origin(url: &str, legacy: &str) -> bool {
    fn strip_slash(value: &str) -> &str {
        value.trim_end_matches('/')
    }
    !url.is_empty() && strip_slash(url) == strip_slash(legacy)
}
