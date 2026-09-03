#![windows_subsystem = "windows"]

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::WindowBuilder,
};
use winreg::enums::*;
use winreg::RegKey;
use wry::WebViewBuilder;

const EMBEDDED_APP_BYTES: &[u8] = include_bytes!("../../dist/DropLink-Portable.exe");
const INSTALLER_HTML: &str = include_str!("installer.html");

enum CustomEvent {
    SendToUi(String),
}

fn main() -> Result<()> {
    let local_appdata = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("C:\\DropLink"));
    let default_install_dir = local_appdata.join("Programs").join("DropLink");

    let event_loop: EventLoop<CustomEvent> = EventLoopBuilder::<CustomEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let window = WindowBuilder::new()
        .with_title("DropLink Setup")
        .with_inner_size(LogicalSize::new(620.0, 440.0))
        .with_resizable(false)
        .build(&event_loop)?;

    let proxy_ipc = proxy.clone();
    let default_dir_str = default_install_dir.to_string_lossy().to_string();

    let webview = WebViewBuilder::new()
        .with_html(INSTALLER_HTML)
        .with_ipc_handler(move |req| {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(req.body()) {
                let cmd = val.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
                match cmd {
                  "do_install" => {
                    let dir_str = val.get("dir").and_then(|v| v.as_str()).unwrap_or("");
                    let desktop = val.get("desktop").and_then(|v| v.as_bool()).unwrap_or(true);
                    let startmenu = val.get("startmenu").and_then(|v| v.as_bool()).unwrap_or(true);
                    let launch = val.get("launch").and_then(|v| v.as_bool()).unwrap_or(true);

                    let install_path = PathBuf::from(dir_str);
                    if let Err(e) = perform_install(&install_path, desktop, startmenu) {
                        eprintln!("Install error: {:#}", e);
                    }

                    let proxy_c = proxy_ipc.clone();
                    let install_path_c = install_path.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(1300));
                        let _ = proxy_c.send_event(CustomEvent::SendToUi("window.__on_install_complete();".to_string()));

                        if launch {
                            let exe = install_path_c.join("DropLink.exe");
                            let _ = open::that(exe);
                        }
                    });
                  }
                  "cancel" => {
                    std::process::exit(0);
                  }
                  "finish" => {
                    std::process::exit(0);
                  }
                  _ => {}
                }
            }
        })
        .build(&window)?;

    // Set default directory in UI
    let init_script = format!("window.__set_default_dir({});", serde_json::to_string(&default_dir_str).unwrap());
    let _ = webview.evaluate_script(&init_script);

    let webview_arc = std::sync::Arc::new(parking_lot::Mutex::new(webview));

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }
            Event::UserEvent(CustomEvent::SendToUi(js)) => {
                if let Some(wv) = webview_arc.try_lock() {
                    let _ = wv.evaluate_script(&js);
                }
            }
            _ => (),
        }
    });
}

fn perform_install(dest_dir: &Path, desktop_shortcut: bool, startmenu_shortcut: bool) -> Result<()> {
    fs::create_dir_all(dest_dir)?;

    let app_exe_path = dest_dir.join("DropLink.exe");
    fs::write(&app_exe_path, EMBEDDED_APP_BYTES)?;

    // Create Uninstaller script in destination directory
    let uninstaller_script = dest_dir.join("Uninstall-DropLink.ps1");
    let uninstaller_content = format!(
        r#"$dest = "{}"
$res = [System.Windows.Forms.MessageBox]::Show("Are you sure you want to completely uninstall DropLink?", "Uninstall DropLink", [System.Windows.Forms.MessageBoxButtons]::YesNo, [System.Windows.Forms.MessageBoxIcon]::Question)
if ($res -eq [System.Windows.Forms.DialogResult]::Yes) {{
    Stop-Process -Name "DropLink" -ErrorAction SilentlyContinue
    Remove-Item "$env:USERPROFILE\Desktop\DropLink.lnk" -ErrorAction SilentlyContinue
    Remove-Item "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\DropLink" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\DropLink" -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item "$dest" -Recurse -Force -ErrorAction SilentlyContinue
    [System.Windows.Forms.MessageBox]::Show("DropLink has been successfully uninstalled.", "DropLink Uninstalled", [System.Windows.Forms.MessageBoxButtons]::OK, [System.Windows.Forms.MessageBoxIcon]::Information)
}}
"#,
        dest_dir.to_string_lossy()
    );
    fs::write(uninstaller_script, uninstaller_content)?;

    // Create uninstaller cmd launcher
    let uninstaller_cmd = dest_dir.join("Uninstall-DropLink.cmd");
    let cmd_content = "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0Uninstall-DropLink.ps1\"\r\n";
    fs::write(uninstaller_cmd, cmd_content)?;

    // Create Desktop Shortcut via PowerShell
    if desktop_shortcut {
        if let Ok(desktop_dir) = std::env::var("USERPROFILE").map(|p| PathBuf::from(p).join("Desktop")) {
            let shortcut_path = desktop_dir.join("DropLink.lnk");
            let ps_script = format!(
                "$WshShell = New-Object -ComObject WScript.Shell; $Shortcut = $WshShell.CreateShortcut('{}'); $Shortcut.TargetPath = '{}'; $Shortcut.Save()",
                shortcut_path.to_string_lossy(),
                app_exe_path.to_string_lossy()
            );
            let _ = Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .output();
        }
    }

    // Create Start Menu Shortcut via PowerShell (both root Programs and subfolder for instant Windows Search)
    if startmenu_shortcut {
        if let Ok(appdata) = std::env::var("APPDATA").map(PathBuf::from) {
            let programs_dir = appdata.join(r"Microsoft\Windows\Start Menu\Programs");
            let sub_dir = programs_dir.join("DropLink");
            let _ = fs::create_dir_all(&sub_dir);
            let root_shortcut = programs_dir.join("DropLink.lnk");
            let sub_shortcut = sub_dir.join("DropLink.lnk");

            let ps_script = format!(
                "$WshShell = New-Object -ComObject WScript.Shell; \
                 $s1 = $WshShell.CreateShortcut('{}'); $s1.TargetPath = '{}'; $s1.Save(); \
                 $s2 = $WshShell.CreateShortcut('{}'); $s2.TargetPath = '{}'; $s2.Save()",
                root_shortcut.to_string_lossy(),
                app_exe_path.to_string_lossy(),
                sub_shortcut.to_string_lossy(),
                app_exe_path.to_string_lossy()
            );
            let _ = Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .output();
        }
    }

    // Register in Windows Add/Remove Programs (Registry)
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok((key, _)) = hkcu.create_subkey(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DropLink") {
        let _ = key.set_value("DisplayName", &"DropLink");
        let _ = key.set_value("DisplayVersion", &"1.0.0");
        let _ = key.set_value("Publisher", &"DropLink Team");
        let _ = key.set_value("InstallLocation", &dest_dir.to_string_lossy().to_string());
        let _ = key.set_value("DisplayIcon", &app_exe_path.to_string_lossy().to_string());
        let uninstall_cmd_str = format!("cmd.exe /c \"{}\"", dest_dir.join("Uninstall-DropLink.cmd").to_string_lossy());
        let _ = key.set_value("UninstallString", &uninstall_cmd_str);
    }

    Ok(())
}
