//! DLL injection module

use std::process::Command;
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId,
};

pub fn find_process(name: &str) -> Result<u32, String> {
    // Use tasklist to find process
    let output = Command::new("tasklist")
        .arg("/FI")
        .arg(format!("IMAGENAME eq {}.exe", name))
        .arg("/NH")
        .output()
        .map_err(|e| format!("Failed to run tasklist: {}", e))?;

    let output = String::from_utf8_lossy(&output.stdout);
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            return parts[1].parse().map_err(|e| format!("Invalid PID: {}", e));
        }
    }

    Err(format!("Process {} not found", name))
}

pub fn inject_dll(pid: u32, dll_path: &str) -> Result<(), String> {
    // TODO: Implement proper DLL injection using CreateRemoteThread
    // For now, use a simple approach with wine load

    let abs_path = std::fs::canonicalize(dll_path)
        .map_err(|e| format!("Failed to resolve DLL path: {}", e))?;

    // Use wine to load the DLL into the process
    let status = Command::new("wine")
        .arg("loadlibrary")
        .arg(&abs_path)
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("wine loadlibrary failed with status: {:?}", s)),
        Err(e) => Err(format!("Failed to run wine: {}", e)),
    }
}

pub fn find_game_window(title: &str) -> Result<HWND, String> {
    unsafe {
        let hwnd = FindWindowW(
            &windows::Win32::UI::WindowsAndMessaging::WC_DIALOG_CLASSW,
            &windows::core::HSTRING::from(title),
        );
        if hwnd.0.is_null() {
            Err(format!("Window '{}' not found", title))
        } else {
            Ok(hwnd)
        }
    }
}
