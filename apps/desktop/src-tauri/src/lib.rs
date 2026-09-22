//! Library entry of the AgentTidy desktop shell.
//!
//! Tauri commands that call the Application API are registered here in
//! Phase 5. The shell stays thin on purpose — `commands.rs` only forwards
//! to `agenttidy-application`; all business logic lives in the workspace
//! crates.

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::doctor,
            commands::scan,
            commands::detect,
            commands::cleanup_preview,
            commands::cleanup_revalidate,
            commands::cleanup_execute,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
