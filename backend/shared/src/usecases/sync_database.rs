use anyhow::{bail, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
use tokio::time::sleep;

use crate::{database::Database, settings::Settings};

const URL_CDN_TRACE: &str = "https://www.cloudflare.com/cdn-cgi/trace";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncResult {
    UpToDate,
    UpdateRequired,
    Updated,
    UpdatedToServer,
}

pub async fn sync_database(
    db: &mut Database,
    settings: &mut Settings,
    accept_migrate_local: bool,
    accept_replace_remote: bool,
) -> Result<SyncResult> {
    let endpoint = settings.api_sync.clone();
    if endpoint.is_none() || endpoint.clone().unwrap().is_empty() {
        bail!("No API sync endpoint configured.");
    }

    let url = endpoint.unwrap();
    let Some((user, password, host_path, root)) = parse_url_info(&url) else {
        bail!("Failed to parse endpoint URL");
    };

    let client = Client::new();
    let dav_base = format!("https://{}/{}", host_path, root.trim_matches('/'));

    let mut t = std::time::Instant::now();
    ensure_webdav_dir(&client, &dav_base, &user, &password).await?;
    println!("WebDAV directory ensured in {:?}", t.elapsed());
    // --- 現在のローカル DB の SHA256 を計算する ---
    let local_sha = sha256_file(&db.filename)?;

    // --- DAV から database.sha256 を読む ---

    t = std::time::Instant::now();
    let remote_sha_opt = dav_read(&client, &dav_base, &user, &password, "database.sha256").await?;
    if let Some(remote_sha_bytes) = remote_sha_opt {
        let remote_sha = String::from_utf8_lossy(&remote_sha_bytes)
            .trim()
            .to_string();

        println!("SHA256 checked in {:?}", t.elapsed());
        // --- SHA256 が同じなら → 最新 ---
        if remote_sha == local_sha {
            return Ok(SyncResult::UpToDate);
        }
    }

    t = std::time::Instant::now();
    let remote_time_opt = dav_read(&client, &dav_base, &user, &password, "database.timez").await?;
    let now = get_current_time().await?;
    println!("Remote time fetched in {:?}", t.elapsed());

    if let Some(remote_time_bytes) = remote_time_opt {
        let remote_time_str = String::from_utf8_lossy(&remote_time_bytes)
            .trim()
            .to_string();
        if let Ok(remote_time) = remote_time_str.parse::<i64>() {
            if accept_replace_remote {
                // continue
            } else if accept_migrate_local {
                // --- ローカルの DB が新しい場合 ---
                let buf = dav_read(&client, &dav_base, &user, &password, "database.db").await?;

                if let Some(buf) = buf {
                    db.hot_replace(&buf).await?;

                    return Ok(SyncResult::Updated);
                } else {
                    bail!("Remote database file not found");
                }
            }
            // --- サーバーの DB が新しい場合 ---
            else if remote_time > now {
                return Ok(SyncResult::UpdateRequired);
            }
        }
    }

    t = std::time::Instant::now();
    // --- ローカルの DB の方が新しい → アップロード ---
    let local_data = fs::read(&db.filename)?;
    dav_write(
        &client,
        &dav_base,
        &user,
        &password,
        "database.db",
        &local_data,
    )
    .await?;
    println!("Database uploaded in {:?}", t.elapsed());

    t = std::time::Instant::now();
    // --- update timez ---
    dav_write(
        &client,
        &dav_base,
        &user,
        &password,
        "database.timez",
        now.to_string().as_bytes(),
    )
    .await?;
    println!("Timez updated in {:?}", t.elapsed());

    // --- update sha256 ---
    t = std::time::Instant::now();
    dav_write(
        &client,
        &dav_base,
        &user,
        &password,
        "database.sha256",
        local_sha.as_bytes(),
    )
    .await?;
    println!("SHA256 updated in {:?}", t.elapsed());

    Ok(SyncResult::UpdatedToServer)
}

async fn get_current_time() -> Result<i64> {
    let client = Client::new();
    let text = client.get(URL_CDN_TRACE).send().await?.text().await?;

    let mut ts: Option<f64> = None;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("ts=") {
            ts = value.parse::<f64>().ok();
            break;
        }
    }

    if let Some(ts) = ts {
        return Ok(ts as i64);
    }

    bail!("Failed to get current time from CDN trace");
}

async fn dav_read(
    client: &Client,
    base: &str,
    user: &str,
    pass: &str,
    file: &str,
) -> Result<Option<Vec<u8>>> {
    let url = format!("{}/{}", base, file);
    let resp = client.get(&url).basic_auth(user, Some(pass)).send().await?;

    if resp.status().is_success() {
        Ok(Some(resp.bytes().await?.to_vec()))
    } else if resp.status().as_u16() == 404 {
        Ok(None)
    } else {
        anyhow::bail!("DAV GET error: {}", resp.status());
    }
}
async fn dav_write(
    client: &Client,
    base: &str,
    user: &str,
    pass: &str,
    file: &str,
    data: &[u8],
) -> Result<()> {
    let url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        file.trim_start_matches('/')
    );
    let max_retries = 5;

    for attempt in 0..max_retries {
        let resp = client
            .put(&url)
            .basic_auth(user, Some(pass))
            .body(data.to_vec())
            .send()
            .await?;

        match resp.status().as_u16() {
            200 | 201 | 204 => {
                return Ok(());
            }
            423 => {
                let wait_ms = 500 + attempt * 200;
                eprintln!(
                    "Resource is locked, retrying in {}ms (attempt {}/{})",
                    wait_ms,
                    attempt + 1,
                    max_retries
                );
                sleep(tokio::time::Duration::from_millis(wait_ms as u64)).await;
                continue;
            }
            s => {
                anyhow::bail!("DAV PUT failed with status: {}", s);
            }
        }
    }

    anyhow::bail!(
        "DAV PUT failed after {} retries due to 423 Locked",
        max_retries
    );
}

fn sha256_file(path: &Path) -> Result<String> {
    let data = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(data);
    Ok(format!("{:x}", hasher.finalize()))
}

fn parse_url_info(endpoint: &str) -> Option<(String, String, String, String)> {
    let url = url::Url::parse(endpoint).ok()?;

    let user = percent_encoding::percent_decode_str(url.username())
        .decode_utf8_lossy()
        .to_string();
    let password = percent_encoding::percent_decode_str(url.password().unwrap_or(""))
        .decode_utf8_lossy()
        .to_string();
    let host = url.host_str()?.to_string();
    let path = url.path().trim_start_matches('/').to_string();
    let mut root = "/".to_string();
    for (k, v) in url.query_pairs() {
        if k == "root" {
            root = v.to_string();
            break;
        }
    }

    Some((user, password, format!("{}/{}", host, path), root))
}

async fn ensure_webdav_dir(
    client: &Client,
    base: &str,
    user: &str,
    pass: &str,
) -> anyhow::Result<()> {
    // 必ず末尾にスラッシュが必要
    let url = base.trim_end_matches('/').to_owned() + "/";

    let res = client
        .request("MKCOL".parse()?, &url)
        .basic_auth(user, Some(pass))
        .send()
        .await?;

    // 既に存在 → 405 / 301 / 200 の場合が多い
    if res.status().is_success() || res.status().as_u16() == 301 || res.status().as_u16() == 405 {
        return Ok(());
    }

    Err(anyhow::anyhow!(
        "Failed to create WebDAV directory: {}",
        res.status()
    ))
}
