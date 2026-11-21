//! Looking for a newer version: the app asks GitHub for the latest release
//! (through the system's own `curl`), and tells the user with a desktop
//! notification and a line in the window. Nothing about the user is sent.

use std::process::Command;
use std::sync::{Arc, Mutex};

/// Where releases are published.
pub const REPO: &str = "lefotugrapher/ridgeline";

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Newer(Release),
    Failed(String),
}

pub type Shared = Arc<Mutex<Status>>;

/// This build's version (RIDGELINE_FAKE_VERSION overrides it, to try the notice out).
pub fn current() -> String {
    std::env::var("RIDGELINE_FAKE_VERSION").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_owned())
}

fn numbers(v: &str) -> Vec<u64> {
    v.trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

/// Whether version `a` is later than `b` ("0.10.0" is later than "0.9.3").
pub fn newer(a: &str, b: &str) -> bool {
    let (x, y) = (numbers(a), numbers(b));
    for k in 0..x.len().max(y.len()) {
        let (p, q) = (x.get(k).copied().unwrap_or(0), y.get(k).copied().unwrap_or(0));
        if p != q {
            return p > q;
        }
    }
    false
}

fn quiet(mut c: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window flashes up
    }
    c
}

/// What GitHub says the latest release is.
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut c = quiet(Command::new(if cfg!(windows) { "curl.exe" } else { "curl" }));
    c.args(["-fsSL", "-m", "12", "-H", "Accept: application/vnd.github+json", "-H", "User-Agent: ridgeline-update-check", &url]);
    let out = c.output().map_err(|e| format!("could not ask ({e})"))?;
    if !out.status.success() {
        return Err("no answer from the release page (offline, or no release yet)".to_owned());
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse(text: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| "the answer was not understood".to_owned())?;
    let version = v["tag_name"].as_str().ok_or("the answer names no version")?.trim_start_matches(['v', 'V']).to_owned();
    let url = v["html_url"].as_str().map(str::to_owned).unwrap_or_else(|| format!("https://github.com/{REPO}/releases/latest"));
    // only ever send people to this project's own pages
    let url = if url.starts_with(&format!("https://github.com/{REPO}/")) { url } else { format!("https://github.com/{REPO}/releases/latest") };
    let title = v["name"].as_str().filter(|s| !s.is_empty()).map(str::to_owned).unwrap_or_else(|| format!("Version {version}"));
    Ok(Release { version, title, url })
}

/// Ask in the background; `ctx` is woken when the answer is in.
pub fn check(shared: &Shared, ctx: eframe::egui::Context) {
    {
        let mut s = shared.lock().unwrap();
        if *s == Status::Checking {
            return;
        }
        *s = Status::Checking;
    }
    let shared = shared.clone();
    let _ = std::thread::Builder::new().name("update-check".into()).spawn(move || {
        let status = match latest() {
            Ok(r) if newer(&r.version, &current()) => Status::Newer(r),
            Ok(_) => Status::UpToDate,
            Err(e) => Status::Failed(e),
        };
        *shared.lock().unwrap() = status;
        ctx.request_repaint();
    });
}

/// A notification on the desktop (outside the app's window).
pub fn notify(title: &str, body: &str) {
    let clean = |s: &str| s.replace(['"', '\\', '\'', '`', '$', '\n', '\r'], " ");
    let (title, body) = (clean(title), clean(body));
    if cfg!(target_os = "macos") {
        let _ = Command::new("osascript").args(["-e", &format!("display notification \"{body}\" with title \"{title}\"")]).spawn();
    } else if cfg!(windows) {
        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
             $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $x = $t.GetElementsByTagName('text'); \
             $x.Item(0).AppendChild($t.CreateTextNode('{title}')) > $null; \
             $x.Item(1).AppendChild($t.CreateTextNode('{body}')) > $null; \
             $id = '{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe'; \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($id).Show([Windows.UI.Notifications.ToastNotification]::new($t))"
        );
        let mut c = quiet(Command::new("powershell.exe"));
        let _ = c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", &script]).spawn();
    } else {
        let _ = Command::new("notify-send").args([&title, &body]).spawn();
    }
}

/// Open a page of this project in the user's browser.
pub fn open_page(url: &str) {
    if !url.starts_with("https://github.com/") {
        return;
    }
    if cfg!(target_os = "macos") {
        let _ = Command::new("open").arg(url).spawn();
    } else if cfg!(windows) {
        let mut c = quiet(Command::new("rundll32.exe"));
        let _ = c.args(["url.dll,FileProtocolHandler", url]).spawn();
    } else {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
