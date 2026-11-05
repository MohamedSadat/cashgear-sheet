#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use cgs_native::{FileRegistry, NativeFile, ReadRange, RecentFile, RecentStore, SaveStore};
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
        .add_filter(
            "Workbooks and CSV",
            &["xlsx", "xlsm", "xltx", "xltm", "csv"],
        )
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

/// Asks where to save, keeping the workbook's own file type, as Excel refuses a file whose name
/// and contents disagree.
#[tauri::command]
async fn pick_save_path(
    app: AppHandle,
    saves: State<'_, SaveStore>,
    suggested: String,
    extension: String,
) -> Result<Option<String>, String> {
    let (sender, receiver) = std::sync::mpsc::channel();
    app.dialog()
        .file()
        .add_filter(
            if extension.eq_ignore_ascii_case("csv") {
                "CSV"
            } else {
                "Excel workbook"
            },
            &[extension.as_str()],
        )
        .set_file_name(&suggested)
        .save_file(move |chosen| {
            let _ = sender.send(chosen);
        });
    let chosen = tauri::async_runtime::spawn_blocking(move || receiver.recv().ok().flatten())
        .await
        .map_err(|error| error.to_string())?;
    let Some(mut path) = chosen.and_then(|chosen| chosen.into_path().ok()) else {
        return Ok(None);
    };
    let matches = path
        .extension()
        .is_some_and(|found| found.to_string_lossy().eq_ignore_ascii_case(&extension));
    if !matches {
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        path.set_file_name(format!("{name}.{extension}"));
    }
    saves.allow(&path)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Starts saving over a file the user opened, or one chosen by `pick_save_path`.
#[tauri::command]
fn begin_save(path: PathBuf, saves: State<'_, SaveStore>) -> Result<String, String> {
    saves.begin(&path)
}

/// Replaces the target with what was written, and makes the new file readable.
#[tauri::command]
fn finish_save(
    token: String,
    size: u64,
    saves: State<'_, SaveStore>,
    files: State<'_, FileRegistry>,
) -> Result<NativeFile, String> {
    let path = saves.finish(&token, size)?;
    files.register(&path)
}

#[tauri::command]
fn abort_save(token: String, saves: State<'_, SaveStore>) -> Result<(), String> {
    saves.abort(&token)
}

fn working_folder(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(data_folder(app)?.join("working-copies"))
}

fn data_folder(app: &AppHandle) -> Result<PathBuf, String> {
    // The smoke test keeps its files apart from the real app's.
    match std::env::var_os("CGS_DESKTOP_SMOKE") {
        Some(folder) => Ok(PathBuf::from(folder)),
        None => app.path().app_data_dir().map_err(|error| error.to_string()),
    }
}

/// Copies the open file somewhere private, for the first edit. See `decisions.md`.
#[tauri::command]
fn make_working_copy(
    id: String,
    app: AppHandle,
    files: State<'_, FileRegistry>,
) -> Result<NativeFile, String> {
    files.working_copy(&id, &working_folder(&app)?)
}

/// Deletes a working copy that is no longer in use.
#[tauri::command]
fn release_working_copy(id: String, files: State<'_, FileRegistry>) {
    if let Some(path) = files.forget(&id) {
        let _ = std::fs::remove_file(path);
    }
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

/// A path the user already opened can be saved over.
#[tauri::command]
fn allow_saving(
    path: PathBuf,
    files: State<'_, FileRegistry>,
    saves: State<'_, SaveStore>,
) -> Result<(), String> {
    if !files.is_registered(&path) {
        return Err("That file isn't open.".to_string());
    }
    saves.allow(&path)
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

/// For the desktop smoke test, in place of the save dialog, under the same variable.
#[tauri::command]
fn allow_save_path(path: PathBuf, saves: State<'_, SaveStore>) -> Result<(), String> {
    if std::env::var_os("CGS_DESKTOP_SMOKE").is_none() {
        return Err("This is only for the smoke test.".to_string());
    }
    saves.allow(&path)
}

fn file_response(status: StatusCode) -> tauri::http::response::Builder {
    // The app origin differs from the protocol's, and the page is cross-origin isolated.
    Response::builder()
        .status(status)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_HEADERS, "Range, Content-Type")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, POST, OPTIONS")
        .header(
            header::ACCESS_CONTROL_EXPOSE_HEADERS,
            "Content-Range, Content-Length, Accept-Ranges",
        )
        .header("Cross-Origin-Resource-Policy", "cross-origin")
}

/// Takes the next bytes of a save, through the protocol, because IPC is four times slower.
fn receive_save(saves: &SaveStore, request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let token = request.uri().path().trim_start_matches('/');
    match saves.append(token, request.body()) {
        Ok(()) => file_response(StatusCode::NO_CONTENT)
            .body(Vec::new())
            .expect("response"),
        Err(message) => file_response(StatusCode::INTERNAL_SERVER_ERROR)
            .body(message.into_bytes())
            .expect("response"),
    }
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
        Err(ReadRange::Changed) => empty(StatusCode::CONFLICT),
    }
}

/// Whether a process is running, by asking the system, with no extra dependency.
fn process_runs(id: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {id}"), "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|output| {
                String::from_utf8_lossy(&output.stdout).contains(&format!("\"{id}\""))
            })
    }
    #[cfg(not(windows))]
    {
        std::path::Path::new(&format!("/proc/{id}")).exists()
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(FileRegistry::default())
        .manage(SaveStore::default())
        .setup(|app| {
            let handle = app.handle();
            let folder = data_folder(handle)?;
            app.manage(RecentStore::new(&folder));
            // Working copies a run left behind. Two windows of this app are two processes, and
            // only a process that has gone leaves copies nobody needs.
            let copies = working_folder(handle)?;
            std::thread::spawn(move || {
                let mine = std::process::id();
                cgs_native::sweep_working_copies(&copies, |owner| {
                    owner == mine || process_runs(owner)
                });
            });
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("cgs-file", |context, request, responder| {
            let app = context.app_handle().clone();
            // File reads must not block the webview's protocol thread.
            std::thread::spawn(move || {
                let response = if request.method() == "POST" {
                    receive_save(&app.state::<SaveStore>(), &request)
                } else {
                    serve_file(&app.state::<FileRegistry>(), &request)
                };
                responder.respond(response);
            });
        })
        .invoke_handler(tauri::generate_handler![
            native_info,
            pick_file,
            recent_files,
            open_recent,
            remember_file,
            open_path,
            allow_saving,
            allow_save_path,
            pick_save_path,
            begin_save,
            finish_save,
            abort_save,
            make_working_copy,
            release_working_copy
        ])
        .run(tauri::generate_context!())
        .expect("the desktop app failed to start");
}
