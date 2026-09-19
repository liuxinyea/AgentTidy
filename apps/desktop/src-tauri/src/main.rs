//! AgentTidy desktop application — Tauri shell.
//!
//! This crate is a thin layer: it only starts the window and forwards
//! commands to the `agenttidy-application` crate. The frontend never
//! accesses agent files or databases directly (architecture red line #1).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    agenttidy_desktop_lib::run()
}
