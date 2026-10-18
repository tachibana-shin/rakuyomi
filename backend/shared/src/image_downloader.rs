use std::{ffi::OsString, fmt::Write as _, path::PathBuf, process::Stdio, time::Duration};

use anyhow::{bail, Context, Result};
use reqwest::{header::HeaderMap, Client, Method, Request, StatusCode};
use tokio::{io::AsyncWriteExt, process::Command, sync::OnceCell};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct ImageResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub bytes: Vec<u8>,
}

impl ImageResponse {
    pub fn error_for_status(self) -> Result<Self> {
        anyhow::ensure!(self.status.is_success(), "HTTP {}", self.status);
        Ok(self)
    }
}

/// Chapter image transfers. Source API calls still belong to their source engine.
/// Custom request bodies/methods and unsupported proxy protocols retain their
/// existing HTTP transport; ordinary GET image bytes are transferred by aria2.
pub struct ImageDownloader {
    program: OsString,
    temporary_root: PathBuf,
    custom_request_client: OnceCell<Client>,
}

impl ImageDownloader {
    pub fn new() -> Result<Self> {
        let executable = std::env::current_exe().context("cannot locate download executable")?;
        let directory = executable
            .parent()
            .context("download executable has no parent")?;
        let bundled = directory.join("aria2c");
        Ok(Self {
            program: if bundled.is_file() {
                bundled.into_os_string()
            } else {
                "aria2c".into()
            },
            // On some readers /tmp is small and RAM-backed. Keep page staging on
            // the writable plugin filesystem, not in the reader's RAM cache.
            temporary_root: directory.to_owned(),
            custom_request_client: OnceCell::new(),
        })
    }

    pub async fn fetch(
        &self,
        token: &CancellationToken,
        request: Request,
    ) -> Result<ImageResponse> {
        anyhow::ensure!(!token.is_cancelled(), "image download cancelled");
        let proxy = crate::tls::proxy_url();
        let custom_proxy = proxy.as_deref().is_some_and(|proxy| {
            url::Url::parse(proxy)
                .map(|url| url.scheme() != "http")
                .unwrap_or(true)
        });
        // aria2 downloads files with GET; do not silently discard a source's
        // POST body, binary header, or SOCKS/HTTPS proxy configuration.
        let custom_request = cfg!(feature = "ffi")
            || request.method() != Method::GET
            || request.body().is_some()
            || request
                .headers()
                .values()
                .any(|value| value.to_str().is_err())
            || custom_proxy;
        if custom_request {
            let client = self
                .custom_request_client
                .get_or_try_init(|| async {
                    crate::tls::client_builder()
                        .timeout(Duration::from_secs(30))
                        .redirect(reqwest::redirect::Policy::none())
                        .build()
                })
                .await?;
            return token
                .run_until_cancelled(async {
                    let response =
                        crate::util::request_with_forced_referer_from_request(client, request, 10)
                            .await?;
                    let status = response.status();
                    let headers = response.headers().clone();
                    let bytes = if status.is_success() {
                        response.bytes().await?.to_vec()
                    } else {
                        Vec::new()
                    };
                    Ok(ImageResponse {
                        status,
                        headers,
                        bytes,
                    })
                })
                .await
                .context("image download cancelled")?;
        }
        self.fetch_with_aria2(token, request, proxy.as_deref())
            .await
    }

    async fn fetch_with_aria2(
        &self,
        token: &CancellationToken,
        request: Request,
        proxy: Option<&str>,
    ) -> Result<ImageResponse> {
        let temporary = tempfile::tempdir_in(&self.temporary_root)
            .context("cannot create private image staging directory")?;
        let output = temporary.path().join("page.bin");
        let log = temporary.path().join("response.log");
        let deadline = request
            .timeout()
            .copied()
            .unwrap_or(Duration::from_secs(30));
        let mut input = String::new();
        writeln!(&mut input, "{}", request.url())?;
        writeln!(&mut input, "  dir={}", temporary.path().display())?;
        input.push_str("  out=page.bin\n");
        for (name, value) in request.headers() {
            writeln!(&mut input, "  header={}: {}", name, value.to_str()?)?;
        }
        if !request.headers().contains_key(reqwest::header::USER_AGENT) {
            writeln!(
                &mut input,
                "  header=User-Agent: rakuyomi/{}",
                env!("CARGO_PKG_VERSION")
            )?;
        }
        if let Some(proxy) = proxy {
            writeln!(&mut input, "  all-proxy={proxy}")?;
        }
        let mut command = Command::new(&self.program);
        command
            .args([
                "--no-conf=true",
                "--no-netrc=true",
                "--input-file=-",
                "--split=1",
                "--max-connection-per-server=1",
                "--max-tries=3",
                "--file-allocation=none",
                "--allow-overwrite=true",
                "--auto-file-renaming=false",
                "--connect-timeout=10",
                "--timeout=30",
                "--http-accept-gzip=true",
                "--check-certificate=true",
                "--quiet=true",
                "--log-level=debug",
            ])
            .arg("--log")
            .arg(&log)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(all(target_os = "linux", target_arch = "arm"))]
        command.arg("--ca-certificate=/etc/ssl/certs/ca-certificates.crt");
        let mut child = command
            .spawn()
            .context("cannot start aria2c image downloader")?;
        let mut stdin = child
            .stdin
            .take()
            .context("aria2c input pipe is unavailable")?;
        // URLs, cookies and other headers go through the private input pipe,
        // never process arguments or public download logs.
        let transfer = async {
            stdin
                .write_all(input.as_bytes())
                .await
                .context("cannot write aria2c input")?;
            drop(stdin);
            child.wait().await.context("cannot wait for aria2c")
        };
        let status = tokio::select! {
            biased;
            _ = token.cancelled() => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                bail!("image download cancelled");
            }
            _ = tokio::time::sleep(deadline) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                bail!("image download timed out");
            }
            status = transfer => status?,
        };
        let (http_status, mut headers) = read_response_headers(&log)
            .await
            .with_context(|| format!("aria2c failed with exit code {:?}", status.code()))?;
        if !status.success() && http_status.is_success() {
            // A 200 header does not mean the body finished: truncated streams,
            // filesystem errors and connection resets must not persist a page.
            bail!(
                "aria2c did not complete the image (exit code {:?})",
                status.code()
            );
        }
        let bytes = if http_status.is_success() {
            tokio::fs::read(&output)
                .await
                .context("cannot read downloaded image")?
        } else {
            Vec::new()
        };
        if headers
            .get(reqwest::header::CONTENT_ENCODING)
            .is_some_and(|value| value == "gzip" || value == "deflate")
        {
            headers.remove(reqwest::header::CONTENT_ENCODING);
            headers.remove(reqwest::header::CONTENT_LENGTH);
        }
        Ok(ImageResponse {
            status: http_status,
            headers,
            bytes,
        })
    }
}

/// aria2 1.37 logs each received HTTP header block before the blank line.
/// Retain only the final response (not redirect/proxy headers). The log lives
/// inside the request's private staging directory and is removed on every exit.
async fn read_response_headers(path: &std::path::Path) -> Result<(StatusCode, HeaderMap)> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let file = tokio::fs::File::open(path)
        .await
        .context("aria2c response log is unavailable")?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    let mut response = None;
    let mut collecting = false;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).await? == 0 {
            break;
        }
        let line = line.strip_suffix(b"\n").unwrap_or(&line);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"HTTP/") {
            let status = std::str::from_utf8(line)?
                .split_ascii_whitespace()
                .nth(1)
                .context("missing HTTP response status")?
                .parse::<u16>()?;
            response = Some((StatusCode::from_u16(status)?, HeaderMap::new()));
            collecting = true;
        } else if collecting && line.is_empty() {
            collecting = false;
        } else if collecting {
            let colon = line
                .iter()
                .position(|byte| *byte == b':')
                .context("invalid HTTP response header")?;
            let name = reqwest::header::HeaderName::from_bytes(&line[..colon])?;
            let value = reqwest::header::HeaderValue::from_bytes(line[colon + 1..].trim_ascii())?;
            if let Some((_, headers)) = &mut response {
                headers.append(name, value);
            }
        }
    }
    response.context("aria2c did not receive an HTTP response")
}

#[cfg(test)]
mod tests;
