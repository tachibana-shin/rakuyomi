use std::time::Duration;
use std::{collections::HashMap, path::PathBuf};
use std::{
    ffi::{CStr, CString},
    io,
    os::raw::c_char,
    sync::OnceLock,
};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::Request as HyperRequest;
use hyper_util::client::legacy::Client;
use hyperlocal::{UnixClientExt, Uri};
use log::error;
use serde::{Deserialize, Serialize};
use tokio::runtime::Runtime;
use tokio::time::timeout;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

static TOKIO_RUNTIME: OnceLock<Runtime> = OnceLock::new();
static TRACING_INIT: std::sync::Once = std::sync::Once::new();

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

fn init_library_logging() {
    TRACING_INIT.call_once(|| {
        if std::env::var("RUST_LOG").is_err() {
            std::env::set_var("RUST_LOG", "info");
        }
        let _ = tracing_subscriber::registry()
            .with(EnvFilter::from_default_env())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(io::stderr)
                    .with_target(true)
                    .with_ansi(false),
            )
            .try_init();
    });
}

#[no_mangle]
pub extern "C" fn uds_request(request_json_ptr: *const c_char) -> *mut c_char {
    init_library_logging();

    if request_json_ptr.is_null() {
        return std::ptr::null_mut();
    }

    let c_str = unsafe { CStr::from_ptr(request_json_ptr) };
    let request_json = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let rt = TOKIO_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    });

    let request_result = rt.block_on(async {
        match serde_json::from_str::<Request>(request_json) {
            Ok(request) => match perform_request(request).await {
                Ok(data) => RequestResult::Response(data),
                Err(e) => {
                    error!("error while performing request: {:?}", e);
                    RequestResult::Error {
                        message: e.to_string(),
                    }
                }
            },
            Err(e) => RequestResult::Error {
                message: format!("while parsing the request: {}", e),
            },
        }
    });

    let response_json = serde_json::to_string(&request_result).unwrap_or_else(|_| {
        r#"{"type":"ERROR","message":"Failed to serialize response"}"#.to_string()
    });
    let c_res = CString::new(response_json).unwrap();
    c_res.into_raw()
}

#[no_mangle]
pub extern "C" fn free_rust_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            let _ = CString::from_raw(ptr);
        }
    }
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
