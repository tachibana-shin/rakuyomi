use std::{env, io};
use std::time::Duration;
use std::{collections::HashMap, path::PathBuf};

use anyhow::{Context, Result};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request as HyperRequest;
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, Uri};
use log::error;
use serde::{Deserialize, Serialize};
use tokio::time::timeout;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Deserialize)]
struct Request {
    socket_path: PathBuf,
    path: String,
    method: String,
    headers: HashMap<String, String>,
    body: String,
    timeout_seconds: f64,
}

#[derive(Debug, Serialize)]
struct ResponseData {
    status: u16,
    body: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum RequestResult {
    #[serde(rename = "ERROR")]
    Error { message: String },
    #[serde(rename = "RESPONSE")]
    Response(ResponseData),
}

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(io::stderr)
                .with_target(true)
                .with_ansi(false),
        )
        .init();

    let request_json = env::args().nth(1).expect("missing json argument");

    let request: Request = serde_json::from_str(request_json.as_str())
        .with_context(|| format!("while parsing the request from stdin: {}", &request_json))?;
    let request_result = match perform_request(request).await {
        Ok(data) => RequestResult::Response(data),
        Err(e) => {
            error!("error while performing request: {:?}", e);

            RequestResult::Error {
                message: e.to_string(),
            }
        }
    };

    println!("{}", serde_json::to_string(&request_result).unwrap());

    Ok(())
}

async fn perform_request(request: Request) -> anyhow::Result<ResponseData> {
    let client = Client::unix();

    let timeout_duration = Duration::from_secs_f64(request.timeout_seconds);
    let response_future = client.request(request.into());
    let response = timeout(timeout_duration, response_future).await??;

    let status = response.status().as_u16();
    let body_bytes = response.collect().await?.to_bytes().to_vec();
    let body = String::from_utf8(body_bytes)?;

    Ok(ResponseData { status, body })
}

impl From<Request> for HyperRequest<Full<Bytes>> {
    fn from(value: Request) -> Self {
        let uri = Uri::new(value.socket_path, value.path.as_str());
        let mut request_builder = HyperRequest::builder()
            .uri(uri)
            .method(value.method.as_str());

        for (k, v) in value.headers {
            request_builder = request_builder.header(k, v);
        }

        request_builder.body(Full::from(value.body)).unwrap()
    }
}
