use std::env;

use anyhow::{bail, Context, Result};
use aws_nitro_enclaves_nsm_api::api::{Request, Response};
use aws_nitro_enclaves_nsm_api::driver::{nsm_exit, nsm_init, nsm_process_request};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_vsock::{VsockAddr, VsockListener, VMADDR_CID_ANY};

const DEFAULT_VSOCK_PORT: u32 = 5005;
const DEFAULT_REPO_URL: &str = "https://github.com/rusyaew/ztinfra-enclaveproducedhtml";
const DEFAULT_PROJECT_REPO_URL: &str = "https://github.com/rusyaew/ztbrowser";
const DEFAULT_WORKLOAD_ID: &str = "ztbrowser-aws-nitro";

#[derive(Deserialize)]
struct EnclaveRequest {
    action: String,
    #[serde(default)]
    nonce_hex: Option<String>,
}

#[derive(Serialize)]
struct EnclaveResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attestation_doc_b64: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let port = env::var("VSOCK_PORT")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(DEFAULT_VSOCK_PORT);

    let listener = VsockListener::bind(VsockAddr::new(VMADDR_CID_ANY, port))
        .with_context(|| format!("Could not bind enclave vsock listener on port {port}"))?;

    println!("Enclave attestation server listening on vsock port {port}");

    loop {
        let (stream, peer_addr) = listener
            .accept()
            .await
            .context("Could not accept vsock connection")?;
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream).await {
                eprintln!("vsock request from {peer_addr:?} failed: {error:#}");
            }
        });
    }
}

async fn handle_connection(stream: tokio_vsock::VsockStream) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut request_line = String::new();
    let read = reader
        .read_line(&mut request_line)
        .await
        .context("Could not read request line")?;

    if read == 0 {
        bail!("Parent closed connection before sending a request");
    }

    let request: EnclaveRequest =
        serde_json::from_str(request_line.trim()).context("Request is not valid JSON")?;
    let response = match request.action.as_str() {
        "index" => EnclaveResponse {
            content_type: Some("text/html; charset=utf-8".to_string()),
            html: Some(render_index_html()),
            attestation_doc_b64: None,
        },
        "attestation" => {
            let nonce_hex = request
                .nonce_hex
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("nonce_hex is required for attestation requests"))?;
            let nonce = parse_nonce_hex(nonce_hex)?;
            let attestation_doc_b64 = request_attestation_doc(nonce)?;
            EnclaveResponse {
                content_type: None,
                html: None,
                attestation_doc_b64: Some(attestation_doc_b64),
            }
        }
        other => bail!("Unsupported action: {other}"),
    };
    let response = serde_json::to_vec(&response)?;

    let stream = reader.get_mut();
    stream
        .write_all(&response)
        .await
        .context("Could not write response JSON")?;
    stream
        .write_all(b"\n")
        .await
        .context("Could not write response terminator")?;

    Ok(())
}

fn render_index_html() -> String {
    let repo_url = env::var("REPO_URL").unwrap_or_else(|_| DEFAULT_REPO_URL.to_string());
    let project_repo_url = env::var("PROJECT_REPO_URL")
        .unwrap_or_else(|_| DEFAULT_PROJECT_REPO_URL.to_string());
    let workload_id = env::var("WORKLOAD_ID").unwrap_or_else(|_| DEFAULT_WORKLOAD_ID.to_string());

    format!(
        "<!DOCTYPE html><html><head><title>ZT infra enclave produced HTML</title></head><body><h1>Hello from Nitro enclave</h1><p>This HTML page was generated inside the enclave and returned to the parent over <code>vsock</code>.</p><ul><li>workload_id: <code>{}</code></li><li>repo_url: <code>{}</code></li><li>project_repo_url: <code>{}</code></li><li>origin: <code>aws nitro enclave</code></li></ul></body></html>",
        workload_id, repo_url, project_repo_url
    )
}

fn request_attestation_doc(nonce: Vec<u8>) -> Result<String> {
    let nsm_fd = nsm_init();
    if nsm_fd < 0 {
        bail!("Could not open /dev/nsm");
    }

    let response = nsm_process_request(
        nsm_fd,
        Request::Attestation {
            user_data: None,
            nonce: Some(ByteBuf::from(nonce)),
            public_key: None,
        },
    );
    nsm_exit(nsm_fd);

    match response {
        Response::Attestation { document } => Ok(BASE64_STANDARD.encode(document)),
        Response::Error(code) => bail!("NSM returned error: {code:?}"),
        other => bail!("Unexpected NSM response: {other:?}"),
    }
}

fn parse_nonce_hex(value: &str) -> Result<Vec<u8>> {
    let clean = value.trim().to_lowercase();
    if clean.is_empty() {
        bail!("nonce_hex must be a non-empty hex string");
    }
    if clean.len() % 2 != 0 || !clean.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("nonce_hex must be an even-length hex string");
    }

    let mut bytes = Vec::with_capacity(clean.len() / 2);
    for index in (0..clean.len()).step_by(2) {
        let pair = &clean[index..index + 2];
        let value =
            u8::from_str_radix(pair, 16).with_context(|| format!("Invalid nonce byte: {pair}"))?;
        bytes.push(value);
    }

    Ok(bytes)
}
