//! Operating-system glue: single instance, the `warpzone://` URI handler,
//! autostart for background mode, desktop notifications.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::Sender;

fn port_file() -> PathBuf {
    warpzone_core::paths::home().join("launcher.port")
}

/// If another launcher runs, hand it our arguments and return `true`.
pub fn forward_to_running(args: &[String]) -> bool {
    let Ok(p) = std::fs::read_to_string(port_file()) else { return false };
    let Ok(port) = p.trim().parse::<u16>() else { return false };
    let Ok(mut s) = TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(300)) else { return false };
    let line = serde_json::to_string(args).unwrap_or_default();
    s.write_all(format!("WARPZONE {line}\n").as_bytes()).is_ok()
}

/// Listen for later starts; each sends its arguments to `tx`.
pub fn listen(tx: Sender<Vec<String>>, wake: impl Fn() + Send + 'static) {
    let Ok(l) = TcpListener::bind("127.0.0.1:0") else { return };
    let Ok(addr) = l.local_addr() else { return };
    let _ = warpzone_core::paths::write_atomic(&port_file(), addr.port().to_string().as_bytes());
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut line = String::new();
            if BufReader::new(s).read_line(&mut line).is_ok()
                && let Some(j) = line.trim().strip_prefix("WARPZONE ")
                && let Ok(args) = serde_json::from_str::<Vec<String>>(j)
            {
                let _ = tx.send(args);
                wake();
            }
        }
    });
}

fn exe() -> String {
    std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "warpzone".into())
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Register `warpzone://` links with the desktop (and a menu entry).
pub fn register_uri_handler() -> std::io::Result<()> {
    if cfg!(windows) {
        let cmd = format!("\"{}\" \"%1\"", exe());
        for args in [
            vec!["add", r"HKCU\Software\Classes\warpzone", "/ve", "/d", "URL:Warpzone", "/f"],
            vec!["add", r"HKCU\Software\Classes\warpzone", "/v", "URL Protocol", "/d", "", "/f"],
            vec!["add", r"HKCU\Software\Classes\warpzone\shell\open\command", "/ve", "/d", cmd.as_str(), "/f"],
        ] {
            std::process::Command::new("reg").args(args).status()?;
        }
        return Ok(());
    }
    let Some(h) = home() else { return Err(std::io::Error::other("no home directory")) };
    let apps = h.join(".local/share/applications");
    std::fs::create_dir_all(&apps)?;
    std::fs::write(
        apps.join("warpzone.desktop"),
        format!(
            "[Desktop Entry]\nType=Application\nName=Warpzone\nComment=Play together, across games\nExec=\"{}\" %u\nIcon=warpzone\nTerminal=false\nCategories=Game;\nMimeType=x-scheme-handler/warpzone;\n",
            exe()
        ),
    )?;
    let _ = std::process::Command::new("xdg-mime").args(["default", "warpzone.desktop", "x-scheme-handler/warpzone"]).status();
    let _ = std::process::Command::new("update-desktop-database").arg(&apps).status();
    Ok(())
}

/// Background mode: start with the session (`true`) or not.
pub fn set_autostart(on: bool) -> std::io::Result<()> {
    if cfg!(windows) {
        let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
        let status = if on {
            std::process::Command::new("reg").args(["add", key, "/v", "Warpzone", "/d", &format!("\"{}\" --background", exe()), "/f"]).status()?
        } else {
            std::process::Command::new("reg").args(["delete", key, "/v", "Warpzone", "/f"]).status()?
        };
        let _ = status;
        return Ok(());
    }
    let Some(h) = home() else { return Ok(()) };
    let f = h.join(".config/autostart/warpzone.desktop");
    if on {
        std::fs::create_dir_all(f.parent().unwrap())?;
        std::fs::write(
            &f,
            format!("[Desktop Entry]\nType=Application\nName=Warpzone (background)\nExec=\"{}\" --background\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n", exe()),
        )?;
    } else if f.exists() {
        std::fs::remove_file(f)?;
    }
    Ok(())
}

pub fn autostart_enabled() -> bool {
    home().is_some_and(|h| h.join(".config/autostart/warpzone.desktop").exists())
}

/// Desktop notification (Linux: notify-send; elsewhere: logged only).
pub fn notify(title: &str, body: &str) {
    if cfg!(target_os = "linux") {
        let _ = std::process::Command::new("notify-send").args(["-a", "Warpzone", title, body]).spawn();
    } else {
        log::info!("{title}: {body}");
    }
}

/// Open a URL or folder with the desktop's default handler.
pub fn open(target: &str) {
    let r = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", target]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(target).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(target).spawn()
    };
    if let Err(e) = r {
        log::warn!("open {target}: {e}");
    }
}
