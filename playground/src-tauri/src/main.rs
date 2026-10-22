// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod models;
mod runner;

use std::sync::Arc;
use tokio::sync::Mutex;
use models::AppItem;
use runner::CancelHandle;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunOutput {
    pub formatted_code: Option<String>,
    pub output: String,
    pub is_error: bool,
    pub elapsed_ms: u64,
}

pub struct AppState {
    pub cancel_handle: Arc<Mutex<Option<CancelHandle>>>,
}

#[tauri::command]
fn load_apps() -> Vec<AppItem> {
    models::load_saved_apps()
}

#[tauri::command]
fn save_apps(apps: Vec<AppItem>) -> Result<(), String> {
    models::save_apps(&apps);
    Ok(())
}

#[tauri::command]
fn get_default_code() -> String {
    models::default_standalone_code()
}

#[tauri::command]
fn resolve_rhp(path: String) -> String {
    runner::resolve_rhp_binary(&path).to_string_lossy().to_string()
}

#[tauri::command]
fn pick_directory() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn pick_file() -> Option<String> {
    rfd::FileDialog::new()
        .pick_file()
        .map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
async fn run_script(
    code: String,
    project_path: String,
    rhp_path: String,
    state: tauri::State<'_, AppState>,
) -> Result<RunOutput, String> {
    let (cancel_handle, cancelled_flag) = CancelHandle::new();
    {
        let mut guard = state.cancel_handle.lock().await;
        *guard = Some(cancel_handle);
    }

    let start = std::time::Instant::now();
    let res = runner::run_script_async(code, project_path, rhp_path, cancelled_flag).await;
    let elapsed = start.elapsed().as_millis() as u64;

    {
        let mut guard = state.cancel_handle.lock().await;
        *guard = None;
    }

    let (output, is_error) = match res.result {
        Ok(out) => (out, false),
        Err(err) => (err, true),
    };

    Ok(RunOutput {
        formatted_code: res.formatted_code,
        output,
        is_error,
        elapsed_ms: elapsed,
    })
}

#[tauri::command]
async fn cancel_run(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.cancel_handle.lock().await;
    if let Some(handle) = guard.take() {
        handle.cancel();
    }
    Ok(())
}

fn main() {
    let state = AppState {
        cancel_handle: Arc::new(Mutex::new(None)),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            load_apps,
            save_apps,
            get_default_code,
            resolve_rhp,
            pick_directory,
            pick_file,
            run_script,
            cancel_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
