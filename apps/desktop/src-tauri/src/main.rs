#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[tauri::command]
fn native_info() -> cgs_native::NativeInfo {
    cgs_native::info()
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![native_info])
        .run(tauri::generate_context!())
        .expect("the desktop app failed to start");
}
