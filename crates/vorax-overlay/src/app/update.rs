//! Updating the overlay from this project's GitHub releases: at start it asks for the latest release, and when it's
//! newer the tray menu offers it with its notes. The new overlay.exe is downloaded next to this one, checked against
//! the SHA-256 GitHub gives for it, and swapped in: a running exe can't be overwritten but can be renamed, so this
//! one moves aside (deleted on the next start) and the new one starts in its place.

use super::sys::note;
use serde_json::Value;
use std::path::{Path, PathBuf};
use windows::core::HSTRING;
use windows::Foundation::{AsyncOperationProgressHandler, Uri};
use windows::Storage::Streams::DataReader;
use windows::Web::Http::{HttpClient, HttpProgress};

pub const REPO: &str = "DinoRaos/vorax-solver-rs";
/// The asset of a release that is the overlay.
const ASSET: &str = "vorax-overlay.exe";

/// This build's version. A test build can claim an older one (VORAX_VERSION at build time) to be offered an update.
pub fn current() -> &'static str {
    option_env!("VORAX_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// what's new, as the release notes say it (without the download instructions and the file table)
    pub notes: String,
    pub page: String,
    url: String,
    size: u64,
    sha256: String,
}

fn version_of(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

fn client() -> Result<HttpClient, String> {
    let c = HttpClient::new().map_err(|e| e.to_string())?;
    if let Ok(h) = c.DefaultRequestHeaders() {
        let _ = h.UserAgent().and_then(|ua| ua.TryParseAdd(&HSTRING::from(format!("vorax-overlay/{}", current()))));
    }
    Ok(c)
}

/// The release notes as plain lines for the card: what's new only, markdown marks dropped.
fn plain_notes(body: &str) -> String {
    let mut out = Vec::new();
    for line in body.lines() {
        let l = line.trim();
        if l.starts_with("Download `") || l.starts_with("Built from") || l.starts_with("| File") {
            break;
        }
        if l.starts_with('>') {
            continue;
        }
        let mut t = l.replace("**", "").replace('`', "");
        // [text](link) → text
        while let (Some(a), Some(b)) = (t.find("]("), t.find('[')) {
            let Some(c) = t[a..].find(')').map(|c| a + c) else { break };
            if b > a {
                break;
            }
            t = format!("{}{}{}", &t[..b], &t[b + 1..a], &t[c + 1..]);
        }
        out.push(t);
    }
    out.join("\n").trim().to_string()
}

/// The latest release when it's newer than this build (None when this is the latest).
pub fn check() -> Result<Option<Release>, String> {
    let c = client()?;
    let uri = Uri::CreateUri(&HSTRING::from(format!("https://api.github.com/repos/{REPO}/releases/latest"))).map_err(|e| e.to_string())?;
    let text = c.GetStringAsync(&uri).and_then(|op| op.get()).map_err(|e| format!("GitHub: {e}"))?.to_string();
    let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("GitHub: no release")?;
    let (Some(latest), Some(mine)) = (version_of(tag), version_of(current())) else { return Err(format!("versions {tag} / {}", current())) };
    if latest <= mine {
        return Ok(None);
    }
    let asset = v["assets"].as_array().and_then(|a| a.iter().find(|x| x["name"] == ASSET)).ok_or(format!("{tag} has no {ASSET}"))?;
    let sha256 = asset["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).ok_or(format!("{tag}: GitHub gives no SHA-256 for {ASSET}"))?;
    Ok(Some(Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: plain_notes(v["body"].as_str().unwrap_or_default()),
        page: v["html_url"].as_str().unwrap_or_default().to_string(),
        url: asset["browser_download_url"].as_str().ok_or("no download link")?.to_string(),
        size: asset["size"].as_u64().unwrap_or(0),
        sha256: sha256.to_ascii_lowercase(),
    }))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn aside(exe: &Path, ext: &str) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(ext);
    PathBuf::from(s)
}

/// Downloads the release's overlay (`progress` from 0 to 1 as it comes), checks it, and swaps it in for this exe.
/// Returns the exe to start.
pub fn install(r: &Release, progress: impl Fn(f32) + Send + Sync + 'static) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let c = client()?;
    let uri = Uri::CreateUri(&HSTRING::from(r.url.as_str())).map_err(|e| e.to_string())?;
    let op = c.GetBufferAsync(&uri).map_err(|e| e.to_string())?;
    let total = r.size.max(1) as f32;
    let _ = op.SetProgress(&AsyncOperationProgressHandler::new(move |_, p: &HttpProgress| {
        progress((p.BytesReceived as f32 / total).min(1.0));
        Ok(())
    }));
    let buf = op.get().map_err(|e| format!("download: {e}"))?;
    let reader = DataReader::FromBuffer(&buf).map_err(|e| e.to_string())?;
    let mut bytes = vec![0u8; buf.Length().map_err(|e| e.to_string())? as usize];
    reader.ReadBytes(&mut bytes).map_err(|e| e.to_string())?;
    let got = sha256_hex(&bytes);
    if got != r.sha256 {
        return Err(format!("the download doesn't match GitHub's SHA-256 ({} bytes): not installed", bytes.len()));
    }
    let new = aside(&exe, ".new");
    std::fs::write(&new, &bytes).map_err(|e| format!("{}: {e}", new.display()))?;
    let old = aside(&exe, ".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("moving this overlay aside: {e}"))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("putting the new overlay in place: {e}"));
    }
    note(&format!("updated to v{} ({} bytes, SHA-256 checked)", r.version, bytes.len()));
    Ok(exe)
}

/// What an update left behind: the overlay it replaced (once that one has closed).
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(aside(&exe, ".old"));
        let _ = std::fs::remove_file(aside(&exe, ".new"));
    }
}
