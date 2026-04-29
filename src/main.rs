use std::env;
use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};

use anyhow::{bail, Context, Result};
use aws_nitro_enclaves_nsm_api::api::{Request, Response};
use aws_nitro_enclaves_nsm_api::driver::{nsm_exit, nsm_init, nsm_process_request};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_vsock::{VsockAddr, VsockListener, VMADDR_CID_ANY};

const DEFAULT_VSOCK_PORT: u32 = 5005;
const DEFAULT_HTTP_PORT: u16 = 9999;
const DEFAULT_BIND_ADDR: &str = "0.0.0.0";
const DEFAULT_COCO_CONFIG_PATH: &str = "/app/coco-runtime-config.json";
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
    if env::var("RUNTIME_MODE").as_deref() == Ok("coco_http")
        || env::var("COCO_HTTP_MODE").as_deref() == Ok("1")
    {
        return run_coco_http_server();
    }

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
    AsyncWriteExt::write_all(stream, &response)
        .await
        .context("Could not write response JSON")?;
    AsyncWriteExt::write_all(stream, b"\n")
        .await
        .context("Could not write response terminator")?;

    Ok(())
}

fn render_index_html() -> String {
    let repo_url = env::var("REPO_URL").unwrap_or_else(|_| DEFAULT_REPO_URL.to_string());
    let project_repo_url =
        env::var("PROJECT_REPO_URL").unwrap_or_else(|_| DEFAULT_PROJECT_REPO_URL.to_string());
    let workload_id = env::var("WORKLOAD_ID").unwrap_or_else(|_| DEFAULT_WORKLOAD_ID.to_string());

    format!(
        "<!DOCTYPE html><html><head><title>ZT infra enclave produced HTML</title></head><body><h1>Hello from Nitro enclave</h1><p>This HTML page was generated inside the enclave and returned to the parent over <code>vsock</code>.</p><ul><li>workload_id: <code>{}</code></li><li>repo_url: <code>{}</code></li><li>project_repo_url: <code>{}</code></li><li>origin: <code>aws nitro enclave</code></li></ul></body></html>",
        workload_id, repo_url, project_repo_url
    )
}

fn run_coco_http_server() -> Result<()> {
    let host = env::var("BIND_ADDR").unwrap_or_else(|_| DEFAULT_BIND_ADDR.to_string());
    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(DEFAULT_HTTP_PORT);
    let listener = TcpListener::bind((host.as_str(), port))
        .with_context(|| format!("Could not bind CoCo HTTP listener on {host}:{port}"))?;

    println!("CoCo attestation server listening on http://{host}:{port}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                std::thread::spawn(|| {
                    if let Err(error) = handle_http_connection(stream) {
                        eprintln!("CoCo HTTP request failed: {error:#}");
                    }
                });
            }
            Err(error) => eprintln!("Could not accept CoCo HTTP connection: {error:#}"),
        }
    }
    Ok(())
}

fn handle_http_connection(mut stream: TcpStream) -> Result<()> {
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 {
        write_http_response(
            &mut stream,
            400,
            "text/plain; charset=utf-8",
            b"bad_request",
        )?;
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    match (method, path) {
        ("GET", "/") => {
            let html = render_coco_index_html();
            write_http_response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                html.as_bytes(),
            )?;
        }
        ("POST", "/.well-known/attestation") => {
            let mut body = vec![0u8; content_length];
            reader.read_exact(&mut body)?;
            let request: Value =
                serde_json::from_slice(if body.is_empty() { b"{}" } else { &body })
                    .context("HTTP request body is not valid JSON")?;
            let nonce = request
                .get("NONCE")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("NONCE is required"))?
                .to_lowercase();
            parse_nonce_hex(&nonce)?;

            let config = load_coco_config()?;
            let evidence = fetch_coco_aa_evidence(
                config
                    .get("aa_evidence_url")
                    .and_then(Value::as_str)
                    .unwrap_or("http://127.0.0.1:8006/aa/evidence"),
                &nonce,
            )?;
            let identity_hint = config
                .get("identity_hint")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| {
                    format!(
                        "coco_image_initdata:{}:{}",
                        config["image_digest"].as_str().unwrap_or_default(),
                        config["initdata_hash"].as_str().unwrap_or_default()
                    )
                });
            let envelope = json!({
                "version": "ztinfra-attestation/v1",
                "service": config["service"],
                "release_id": config["release_id"],
                "platform": config.get("platform").and_then(Value::as_str).unwrap_or("aws_coco_snp"),
                "nonce": nonce,
                "claims": {
                    "workload_pubkey": config.get("workload_pubkey").cloned().unwrap_or(Value::Null),
                    "identity_hint": identity_hint,
                },
                "evidence": {
                    "type": "coco_trustee_evidence",
                    "payload": evidence,
                },
                "facts_url": config.get("facts_url").cloned().unwrap_or(Value::Null),
            });
            let payload = serde_json::to_vec(&envelope)?;
            write_http_response(&mut stream, 200, "application/json", &payload)?;
        }
        _ => write_http_response(&mut stream, 404, "text/plain; charset=utf-8", b"not_found")?,
    }

    Ok(())
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn render_coco_index_html() -> String {
    env::var("COCO_INDEX_HTML").unwrap_or_else(|_| {
        "<!DOCTYPE html><html><head><title>ZT infra enclave produced HTML</title></head><body><h1>Hello from CoCo workload</h1><p>This HTML page was generated by the same canonical workload container lowered for AWS CoCo.</p></body></html>".to_string()
    })
}

fn load_coco_config() -> Result<Value> {
    let path = env::var("COCO_RUNTIME_CONFIG_PATH")
        .unwrap_or_else(|_| DEFAULT_COCO_CONFIG_PATH.to_string());
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Could not read CoCo runtime config: {path}"))?;
    serde_json::from_str(&contents).context("CoCo runtime config is not valid JSON")
}

fn fetch_coco_aa_evidence(aa_evidence_url: &str, nonce_hex: &str) -> Result<Value> {
    let parsed = aa_evidence_url
        .strip_prefix("http://")
        .ok_or_else(|| anyhow::anyhow!("only http:// AA evidence URLs are supported"))?;
    let (authority, path) = parsed.split_once('/').unwrap_or((parsed, ""));
    let (host, port) = if let Some((host, port)) = authority.split_once(':') {
        (host, port.parse::<u16>()?)
    } else {
        (authority, 80)
    };
    let base_path = format!("/{path}");
    let separator = if base_path.contains('?') { '&' } else { '?' };
    let request_path = format!("{base_path}{separator}runtime_data={nonce_hex}");

    let mut stream = TcpStream::connect((host, port))
        .with_context(|| format!("Could not connect to CoCo AA endpoint {host}:{port}"))?;
    write!(
        stream,
        "GET {request_path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let response_text = String::from_utf8(response).context("AA response is not UTF-8")?;
    let (headers, body) = response_text
        .split_once("\r\n\r\n")
        .ok_or_else(|| anyhow::anyhow!("AA response is not valid HTTP"))?;
    if !headers.starts_with("HTTP/1.1 200") && !headers.starts_with("HTTP/1.0 200") {
        bail!(
            "AA endpoint returned non-200 response: {}",
            headers.lines().next().unwrap_or(headers)
        );
    }
    serde_json::from_str(body).context("AA evidence response is not valid JSON")
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
