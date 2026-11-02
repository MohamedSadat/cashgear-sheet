#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use cgs_native::{FileRegistry, NativeFile, ReadRange, RecentFile, RecentStore};
use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
fn native_info() -> cgs_native::NativeInfo {
    cgs_native::info()
}

/// Asks for a workbook, without blocking the UI thread the dialog needs.
#[tauri::command]
async fn pick_file(
    app: AppHandle,
    files: State<'_, FileRegistry>,
) -> Result<Option<NativeFile>, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter("Excel workbooks", &["xlsx", "xlsm", "xltx", "xltm"])
        .pick_file(move |chosen| {
            let _ = sender.send(chosen);
        });
    let chosen = tauri::async_runtime::spawn_blocking(move || receiver.recv().ok().flatten())
        .await
        .map_err(|error| error.to_string())?;
    let Some(path) = chosen.and_then(|chosen| chosen.into_path().ok()) else {
        return Ok(None);
    };
    files.register(&path).map(Some)
}

#[tauri::command]
fn recent_files(store: State<'_, RecentStore>) -> Vec<RecentFile> {
    store.list()
}

/// Opens a file from the recent list, and nothing else: the id must be on the list.
#[tauri::command]
fn open_recent(
    id: String,
    files: State<'_, FileRegistry>,
    store: State<'_, RecentStore>,
) -> Result<Option<NativeFile>, String> {
    let path = Path::new(&id);
    if !store.contains(path) || !path.is_file() {
        return Ok(None);
    }
    files.register(path).map(Some)
}

#[tauri::command]
fn remember_file(path: PathBuf, store: State<'_, RecentStore>) -> Result<(), String> {
    store.remember(&path)
}

/// For the desktop smoke test, which can't drive a native dialog. It needs the variable the
/// test sets, so the webview can't use it to read files nobody chose.
#[tauri::command]
fn open_path(path: PathBuf, files: State<'_, FileRegistry>) -> Result<NativeFile, String> {
    if std::env::var_os("CGS_DESKTOP_SMOKE").is_none() {
        return Err("This is only for the smoke test.".to_string());
    }
    files.register(&path)
}

fn file_response(status: StatusCode) -> tauri::http::response::Builder {
    // The app origin differs from the protocol's, and the page is cross-origin isolated.
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Content-Length, Accept-Ranges",
        )
        .header("Cross-Origin-Resource-Policy", "cross-origin")
}

fn serve_file(files: &FileRegistry, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let empty = |status| file_response(status).body(Vec::new()).expect("response");
    if request.method() == "OPTIONS" {
        return empty(StatusCode::NO_CONTENT);
    }
    let id = request.uri().path().trim_start_matches('/');
    let range = request
        .headers()
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());
    let Some((start, end)) = cgs_native::parse_range(range) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    match files.read_range(id, start, end) {
        Ok((body, size)) => {
            let last = start + body.len() as u64 - 1;
            file_response(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, "application/octet-stream")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {start}-{last}/{size}"),
                )
                .header(header::CONTENT_LENGTH, body.len())
                .body(body)
                .expect("response")
        }
        Err(ReadRange::Unsatisfiable(size)) => file_response(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{size}"))
            .body(Vec::new())
            .expect("response"),
        Err(ReadRange::NotFound) => empty(StatusCode::NOT_FOUND),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(FileRegistry::default())
        .setup(|app| {
            // The smoke test keeps its files apart from the real app's list.
            let folder = match std::env::var_os("CGS_DESKTOP_SMOKE") {
                Some(folder) => PathBuf::from(folder),
                None => app.path().app_data_dir()?,
            };
            app.manage(RecentStore::new(&folder));
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("cgs-file", |context, request, responder| {
            let app = context.app_handle().clone();
            // File reads must not block the webview's protocol thread.
            std::thread::spawn(move || {
                let files = app.state::<FileRegistry>();
                responder.respond(serve_file(&files, &request));
            });
        })
        .invoke_handler(tauri::generate_handler![
            native_info,
            pick_file,
            recent_files,
            open_recent,
            remember_file,
            open_path
        ])
        .run(tauri::generate_context!())
        .expect("the desktop app failed to start");
}
