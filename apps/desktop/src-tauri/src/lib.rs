//! Library entry of the AgentTidy desktop shell.
//!
//! Tauri commands that call the Application API will be registered here
//! in Phase 5. The shell stays thin on purpose.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
