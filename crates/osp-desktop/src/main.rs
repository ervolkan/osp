//! OSP Desktop — HTTP server + static frontend.
//!
//! Çalıştırma: cargo run -p osp-desktop
//! Browser: http://localhost:7878
//!
//! Tauri'ye migration: bu server'ı `tauri::Builder` ile değiştir,
//! command handler'lar (`lib.rs`) aynı kalır.

use std::path::PathBuf;

use serde_json::Value;
use tiny_http::{Header, Method, Response, Server};

const PORT: u16 = 7878;
/// Frontend dizini — crate köküne göre absolute (CWD'den bağımsız).
const FRONTEND_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend");

fn main() {
    let addr = format!("127.0.0.1:{PORT}");
    let server = Server::http(&addr).expect("Failed to bind");
    println!("╔════════════════════════════════════════════╗");
    println!("║  OSP Desktop v0.1                          ║");
    println!("║  Open: http://localhost:{}              ║", PORT);
    println!("║  Press Ctrl+C to stop                      ║");
    println!("╚════════════════════════════════════════════╝");

    for request in server.incoming_requests() {
        handle_request(request);
    }
}

fn handle_request(mut request: tiny_http::Request) {
    let url = request.url().to_string();
    let method = request.method().clone();

    match (&method, url.as_str()) {
        // ── API routes ──
        (Method::Get, "/api/health") => {
            let body = serde_json::json!({ "status": osp_desktop::cmd_health() });
            respond_json(request, &body);
        }

        (Method::Post, "/api/analyze") => {
            let mut content = String::new();
            request.as_reader().read_to_string(&mut content).ok();

            let req: Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(e) => {
                    respond_error(request, 400, &format!("Invalid JSON: {e}"));
                    return;
                }
            };

            let repo_path = req["repo_path"].as_str().unwrap_or("");
            let scip_path = req["scip_path"].as_str();

            if repo_path.is_empty() {
                respond_error(request, 400, "repo_path is required");
                return;
            }

            match osp_desktop::cmd_analyze_repo(repo_path, scip_path) {
                Ok(result) => respond_json(request, &result),
                Err(e) => respond_error(request, 500, &e),
            }
        }

        // ── Static files ──
        (Method::Get, "/") => serve_file(request, "index.html", "text/html"),
        (Method::Get, path) if path.starts_with('/') => {
            serve_file(request, &path[1..], guess_mime(path));
        }

        _ => respond_error(request, 404, "Not found"),
    }
}

fn serve_file(request: tiny_http::Request, filename: &str, mime: &str) {
    let path = PathBuf::from(FRONTEND_DIR).join(filename);

    match std::fs::read(&path) {
        Ok(data) => {
            let response = Response::from_data(data)
                .with_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap())
                .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], b"*").unwrap());
            request.respond(response).ok();
        }
        Err(_) => respond_error(request, 404, &format!("File not found: {filename}")),
    }
}

fn respond_json<T: Serialize>(request: tiny_http::Request, data: &T) {
    let body = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    let response = Response::from_string(body)
        .with_header(Header::from_bytes(&b"Content-Type"[..], b"application/json".as_ref()).unwrap())
        .with_header(Header::from_bytes(&b"Access-Control-Allow-Origin"[..], b"*").unwrap());
    request.respond(response).ok();
}

fn respond_error(request: tiny_http::Request, code: u16, message: &str) {
    let body = serde_json::json!({ "error": message });
    let response = Response::from_string(body.to_string())
        .with_status_code(code)
        .with_header(Header::from_bytes(&b"Content-Type"[..], b"application/json".as_ref()).unwrap());
    request.respond(response).ok();
}

fn guess_mime(path: &str) -> &'static str {
    if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".json") {
        "application/json"
    } else if path.ends_with(".png") {
        "image/png"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else {
        "application/octet-stream"
    }
}

use serde::Serialize;
