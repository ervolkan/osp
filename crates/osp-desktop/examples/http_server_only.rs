//! Standalone HTTP server — Tauri native window AÇMAZ, sadece API + frontend
//! static dosyaları sunar. Görsel doğrulama (Playwright/screenshot) için.
//!
//! cargo run --example http_server_only --package osp-desktop
//! Sonra tarayıcıda http://localhost:7878 → svelte repo path gir → Analyze.

use std::path::PathBuf;

use serde_json::Value;
use tiny_http::{Header, Method, Response, Server};

use serde::Serialize;

const PORT: u16 = 7878;
const FRONTEND_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/frontend");

fn main() {
    let addr = format!("127.0.0.1:{PORT}");
    let server = Server::http(&addr).expect("Failed to bind API server");
    eprintln!("Standalone HTTP server on http://localhost:{PORT} (no Tauri window)");
    eprintln!("Frontend + /api/* served. Ctrl+C to stop.");
    for request in server.incoming_requests() {
        handle_request(request);
    }
}

// main.rs'teki handle_request'in aynısı — Tauri olmadan API + static.
fn handle_request(mut request: tiny_http::Request) {
    let url = request.url().to_string();
    let method = request.method().clone();

    match (&method, url.as_str()) {
        (Method::Get, "/api/health") => {
            let body = serde_json::json!({ "status": osp_desktop::cmd_health() });
            respond_json(request, &body);
        }
        (Method::Get, "/api/vision") => {
            respond_json(request, &osp_desktop::cmd_get_vision_config());
        }
        (Method::Post, "/api/detect-scip") => {
            let body = read_json_body(&mut request);
            let repo = body["repo_path"].as_str().unwrap_or("");
            if repo.is_empty() {
                respond_error(request, 400, "repo_path is required");
            } else {
                let scip_path = osp_desktop::cmd_detect_scip(repo);
                respond_json(request, &serde_json::json!({ "scip_path": scip_path }));
            }
        }
        (Method::Post, "/api/stats") => {
            let body = read_json_body(&mut request);
            let repo = body["repo"].as_str().unwrap_or("");
            if repo.is_empty() {
                respond_error(request, 400, "repo is required");
            } else {
                match osp_desktop::cmd_get_repo_stats(repo) {
                    Ok(stats) => respond_json(request, &stats),
                    Err(e) => respond_error(request, 500, &e),
                }
            }
        }
        (Method::Post, "/api/analyze") => {
            let body = read_json_body(&mut request);
            let repo_path = body["repo_path"].as_str().unwrap_or("");
            let scip_path = body["scip_path"].as_str();
            if repo_path.is_empty() {
                respond_error(request, 400, "repo_path is required");
            } else {
                match osp_desktop::cmd_analyze_repo(repo_path, scip_path) {
                    Ok(result) => respond_json(request, &result),
                    Err(e) => respond_error(request, 500, &e),
                }
            }
        }
        (Method::Get, "/") => serve_file(request, "index.html", "text/html"),
        (Method::Get, path) if path.starts_with('/') => {
            serve_file(request, &path[1..], guess_mime(path));
        }
        _ => respond_error(request, 404, "Not found"),
    }
}

fn read_json_body(request: &mut tiny_http::Request) -> Value {
    let mut content = String::new();
    request.as_reader().read_to_string(&mut content).ok();
    serde_json::from_str(&content).unwrap_or(Value::Null)
}

fn serve_file(request: tiny_http::Request, filename: &str, mime: &str) {
    let path = PathBuf::from(FRONTEND_DIR).join(filename);
    match std::fs::read(&path) {
        Ok(data) => {
            let response = Response::from_data(data)
                .with_header(Header::from_bytes(&b"Content-Type"[..], mime.as_bytes()).unwrap())
                .with_header(
                    Header::from_bytes(&b"Access-Control-Allow-Origin"[..], b"*").unwrap(),
                );
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
