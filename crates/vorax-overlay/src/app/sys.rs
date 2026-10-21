//! Windows itself: the game's window (by its title), where its picture is on screen, whether it's in front, and
//! where the game writes UE_game.log. Plus the overlay's own debug log and settings.

use std::path::{Path, PathBuf};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClientRect, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible};

/// The game's window title.
const GAME_TITLE: &str = "Torchlight: Infinite";
/// Torchlight: Infinite on Steam.
const STEAM_APP: &str = "1974050";

/// A window handle that can cross threads (it's only a number to Windows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hwnd(pub isize);

impl Hwnd {
    pub fn get(self) -> HWND {
        HWND(self.0 as *mut core::ffi::c_void)
    }
    pub fn of(h: HWND) -> Hwnd {
        Hwnd(h.0 as isize)
    }
}

unsafe extern "system" fn each_window(h: HWND, found: LPARAM) -> BOOL {
    let found = &mut *(found.0 as *mut Option<Hwnd>);
    if !IsWindowVisible(h).as_bool() {
        return true.into();
    }
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, &mut buf);
    if n <= 0 {
        return true.into();
    }
    let title = String::from_utf16_lossy(&buf[..n as usize]);
    if title.trim_end() != GAME_TITLE {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(h, Some(&mut pid));
    if pid == std::process::id() {
        return true.into();
    }
    *found = Some(Hwnd::of(h));
    false.into()
}

/// The game's shown top-level window, by its title.
pub fn find_game_window() -> Option<Hwnd> {
    let mut found: Option<Hwnd> = None;
    unsafe {
        let _ = EnumWindows(Some(each_window), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// The game's picture on screen (its client area): x, y, width, height in physical pixels.
pub fn client_rect(h: Hwnd) -> Option<(i32, i32, i32, i32)> {
    unsafe {
        if IsIconic(h.get()).as_bool() {
            return None;
        }
        let mut r = RECT::default();
        GetClientRect(h.get(), &mut r).ok()?;
        let mut p = POINT { x: 0, y: 0 };
        if !ClientToScreen(h.get(), &mut p).as_bool() {
            return None;
        }
        let (w, hh) = (r.right - r.left, r.bottom - r.top);
        (w > 0 && hh > 0).then_some((p.x, p.y, w, hh))
    }
}

pub fn foreground() -> Hwnd {
    Hwnd::of(unsafe { GetForegroundWindow() })
}

fn reg_string(root: HKEY, key: PCWSTR, value: PCWSTR) -> Option<String> {
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let err = unsafe { RegGetValueW(root, key, value, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len)) };
    if err.is_err() {
        return None;
    }
    let n = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..n]))
}

fn quoted_values<'a>(text: &'a str, key: &str) -> Vec<&'a str> {
    let pat = format!("\"{key}\"");
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix(&pat)?.trim();
            rest.strip_prefix('"')?.strip_suffix('"')
        })
        .collect()
}

const LOG_IN_GAME: [&str; 5] = ["UE_game", "TorchLight", "Saved", "Logs", "UE_game.log"];

/// Where the game writes UE_game.log: under each Steam library that has the game, or the launcher's folders.
/// The newest log wins; without one yet, where it will appear.
pub fn find_log() -> Option<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for (root, key, value) in [
        (HKEY_CURRENT_USER, w!("Software\\Valve\\Steam"), w!("SteamPath")),
        (HKEY_LOCAL_MACHINE, w!("SOFTWARE\\WOW6432Node\\Valve\\Steam"), w!("InstallPath")),
        (HKEY_LOCAL_MACHINE, w!("SOFTWARE\\Valve\\Steam"), w!("InstallPath")),
    ] {
        if let Some(p) = reg_string(root, key, value) {
            roots.push(PathBuf::from(p.replace('/', "\\")));
        }
    }
    roots.push(PathBuf::from("C:\\Program Files (x86)\\Steam"));
    let mut libraries: Vec<PathBuf> = roots.clone();
    for r in &roots {
        if let Ok(vdf) = std::fs::read_to_string(r.join("steamapps").join("libraryfolders.vdf")) {
            for p in quoted_values(&vdf, "path") {
                libraries.push(PathBuf::from(p.replace("\\\\", "\\")));
            }
        }
    }
    let same = |a: &Path, b: &Path| a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase();
    let mut unique: Vec<PathBuf> = Vec::new();
    for l in libraries {
        if !unique.iter().any(|u| same(u, &l)) {
            unique.push(l);
        }
    }
    let mut candidates: Vec<PathBuf> = Vec::new();
    for lib in &unique {
        let steamapps = lib.join("steamapps");
        if let Ok(acf) = std::fs::read_to_string(steamapps.join(format!("appmanifest_{STEAM_APP}.acf"))) {
            if let Some(dir) = quoted_values(&acf, "installdir").first() {
                candidates.push(LOG_IN_GAME.iter().fold(steamapps.join("common").join(dir), |p, s| p.join(s)));
            }
        }
    }
    for base in ["C:\\Program Files (x86)\\Torchlight Infinite", "C:\\Program Files\\Torchlight Infinite"] {
        candidates.push(LOG_IN_GAME.iter().fold(PathBuf::from(base).join("Game"), |p, s| p.join(s)));
    }
    // a game under Program Files that can't write there has its log in Windows' VirtualStore copy
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        for c in candidates.clone() {
            let s = c.to_string_lossy().to_string();
            if s.to_lowercase().starts_with("c:\\program files") {
                candidates.push(PathBuf::from(&local).join("VirtualStore").join(&s[3..]));
            }
        }
    }
    let mut there: Vec<(PathBuf, std::time::SystemTime)> = candidates.iter().filter_map(|p| std::fs::metadata(p).ok().filter(|m| m.is_file()).map(|m| (p.clone(), m.modified().unwrap_or(std::time::UNIX_EPOCH)))).collect();
    there.sort_by(|a, b| b.1.cmp(&a.1));
    if let Some((p, _)) = there.into_iter().next() {
        return Some(p);
    }
    candidates.into_iter().find(|p| p.ancestors().nth(3).is_some_and(|d| d.exists()))
}

/// %APPDATA%\VoraxRs: settings and the debug log.
pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let d = base.join("VoraxRs");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// A line in the debug log (%APPDATA%\VoraxRs\overlay.log), kept under 2 MB.
pub fn note(msg: &str) {
    use std::io::Write;
    let path = data_dir().join("overlay.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 2_000_000) {
        let _ = std::fs::rename(&path, data_dir().join("overlay.log.old"));
    }
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let line = format!("{secs:.3} {msg}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
    if cfg!(debug_assertions) || std::env::var_os("VORAX_CONSOLE").is_some() {
        eprint!("{line}");
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    /// Intern Shelley level: sets the instrument rerolls a game starts with
    pub shelley: i32,
    /// the overlay shown (Ctrl+Alt+V)
    pub shown: bool,
    /// read card titles with Windows' text reader (else by their pictures)
    pub ocr: bool,
    /// the marks' colour: an index into draw::ACCENTS
    #[serde(default)]
    pub accent: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { shelley: 6, shown: true, ocr: true, accent: 0 }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read_to_string(data_dir().join("settings.json")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::write(data_dir().join("settings.json"), serde_json::to_string_pretty(self).unwrap());
    }
}
