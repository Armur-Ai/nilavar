//! Nilavar desktop application entry point.

#![deny(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    vault_desktop::run()
}
