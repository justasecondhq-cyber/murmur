//! OS-specific bits: focused app detection, permission checks, opening links.

use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct FocusedApp {
    pub name: String,
    pub title: String,
}

pub fn focused_app() -> FocusedApp {
    match active_win_pos_rs::get_active_window() {
        Ok(w) => FocusedApp { name: w.app_name, title: w.title },
        Err(_) => FocusedApp::default(),
    }
}

#[cfg(target_os = "macos")]
mod mac {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    pub fn accessibility_trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Permissions {
    pub platform: &'static str,
    /// Needed on macOS to send the paste keystroke and read selections.
    pub accessibility: Option<bool>,
    pub microphone_devices: usize,
    pub wayland: bool,
}

pub fn permissions() -> Permissions {
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    };
    #[cfg(target_os = "macos")]
    let accessibility = Some(mac::accessibility_trusted());
    #[cfg(not(target_os = "macos"))]
    let accessibility: Option<bool> = None;
    Permissions {
        platform,
        accessibility,
        microphone_devices: crate::audio::list_input_devices().len(),
        wayland: std::env::var("WAYLAND_DISPLAY").is_ok(),
    }
}

/// Open a URL or folder with the system handler.
pub fn open(target: &str) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(target).spawn();
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("explorer").arg(target).creation_flags(CREATE_NO_WINDOW).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(target).spawn();
}

/// Deep link to the relevant privacy pane.
pub fn open_permission_settings(kind: &str) {
    #[cfg(target_os = "macos")]
    {
        let url = match kind {
            "microphone" => "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            _ => "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
        };
        open(url);
    }
    #[cfg(target_os = "windows")]
    {
        let _ = kind;
        open("ms-settings:privacy-microphone");
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = kind;
    }
}
