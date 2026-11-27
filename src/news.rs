//! Messages from the author ("news").
//!
//! The author publishes a small file, `news.json`, in this project's GitHub repository. The app
//! reads it now and then (through the system's own `curl`, like the update check) and shows new
//! messages once: a desktop notification and a line in the window.
//!
//! Safety:
//! - Every message is signed (Ed25519) with the author's private key, which never leaves the
//!   author's computer. The public half is built into the app. A message whose signature does
//!   not check out is ignored, so nobody else can make the app show anything, even with access
//!   to the GitHub repository.
//! - Plain text only, with length limits; links only to a short list of sites; every message has
//!   an expiry date; each message is shown once.
//! - Nothing about the user is sent. The app only downloads one public file.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Where the messages are published.
pub const FEED: &str = "https://raw.githubusercontent.com/lefotugrapher/kayzee-3d/main/news.json";

/// The author's public key (hex). Messages must be signed by the matching private key.
pub const PUBLIC_KEY_HEX: &str = "48d847327f46aa047bcd3c0660aca40092f574f2ce3630e8c8fbefb20f210402";

const MAX_TITLE: usize = 80;
const MAX_BODY: usize = 400;
const MAX_DAYS: u64 = 120;
/// Sites a message may link to.
const LINK_HOSTS: &[&str] = &["github.com", "www.youtube.com", "youtube.com", "youtu.be", "www.reddit.com", "reddit.com"];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub link: String,
    /// Seconds since 1970 after which the message is no longer shown.
    pub expires: u64,
    /// Ed25519 signature (hex) over `signed_bytes`.
    pub sig: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Feed {
    #[serde(default)]
    pub messages: Vec<Message>,
}

pub type Shared = Arc<Mutex<Vec<Message>>>;

/// The exact bytes that are signed: every field, length-prefixed so nothing can be shifted between fields.
pub fn signed_bytes(m: &Message) -> Vec<u8> {
    let mut out = b"kayzee3d-news-v1".to_vec();
    for f in [m.id.as_str(), m.title.as_str(), m.body.as_str(), m.link.as_str(), &m.expires.to_string()] {
        out.extend_from_slice(format!("|{}:", f.len()).as_bytes());
        out.extend_from_slice(f.as_bytes());
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 || !s.is_ascii() {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

fn public_key() -> Option<VerifyingKey> {
    let b: [u8; 32] = unhex(PUBLIC_KEY_HEX)?.try_into().ok()?;
    VerifyingKey::from_bytes(&b).ok()
}

/// Only plain https links to a few known sites.
pub fn link_ok(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    let Some(rest) = url.strip_prefix("https://") else { return false };
    if url.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'' || c == '\\' || c == '`') {
        return false;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.contains('@') || host.contains(':') {
        return false;
    }
    if host == "github.com" {
        // GitHub links only to this author's pages
        return rest.starts_with("github.com/lefotugrapher/") || rest == "github.com/lefotugrapher";
    }
    LINK_HOSTS.contains(&host)
}

fn text_ok(s: &str, max: usize) -> bool {
    let n = s.chars().count();
    n > 0 && n <= max && !s.chars().any(|c| c.is_control())
}

/// Check one message: shape, limits, expiry and the signature. `Ok` means it may be shown.
pub fn check_message(m: &Message, key: &VerifyingKey, now: u64) -> Result<(), String> {
    if m.id.is_empty() || m.id.len() > 64 || !m.id.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)) {
        return Err("bad id".into());
    }
    if !text_ok(&m.title, MAX_TITLE) {
        return Err(format!("title must be 1 to {MAX_TITLE} characters, one line"));
    }
    if !text_ok(&m.body, MAX_BODY) {
        return Err(format!("text must be 1 to {MAX_BODY} characters, one line"));
    }
    if !link_ok(&m.link) {
        return Err("link not allowed".into());
    }
    if m.expires <= now {
        return Err("expired".into());
    }
    if m.expires > now + MAX_DAYS * 86_400 + 86_400 {
        return Err("expiry too far ahead".into());
    }
    let sig: [u8; 64] = unhex(&m.sig).and_then(|v| v.try_into().ok()).ok_or("bad signature format")?;
    key.verify_strict(&signed_bytes(m), &Signature::from_bytes(&sig)).map_err(|_| "signature does not match".to_string())
}

/// Messages from the feed text that check out and have not been seen yet (oldest first, at most 3).
pub fn fresh(text: &str, seen: &[String], now: u64) -> Vec<Message> {
    let Some(key) = public_key() else { return vec![] };
    let Ok(feed) = serde_json::from_str::<Feed>(text) else { return vec![] };
    let mut out: Vec<Message> = feed.messages.into_iter().filter(|m| !seen.contains(&m.id) && check_message(m, &key, now).is_ok()).collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.truncate(3);
    out
}

fn fetch() -> Result<String, String> {
    let url = std::env::var("KAYZEE3D_NEWS_URL").ok().filter(|u| u.starts_with("file://") || u.starts_with("https://")).unwrap_or_else(|| FEED.to_owned());
    let mut c = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.args(["-fsSL", "-m", "12", "--max-filesize", "65536", "--proto", "=https,file", "-H", "User-Agent: kayzee3d-news", &url]);
    let out = c.output().map_err(|e| format!("could not ask ({e})"))?;
    if !out.status.success() {
        return Err("no answer".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Look in the background; new messages land in `shared` and `ctx` is woken.
pub fn check(shared: &Shared, seen: Vec<String>, ctx: eframe::egui::Context) {
    let shared = shared.clone();
    let _ = std::thread::Builder::new().name("news-check".into()).spawn(move || {
        if let Ok(text) = fetch() {
            let new = fresh(&text, &seen, crate::update::now_secs());
            if !new.is_empty() {
                shared.lock().unwrap().extend(new);
                ctx.request_repaint();
            }
        }
    });
}

// ---------- the author's side: making a key and signing messages ----------

/// Where the author's private key lives: outside the repository, on the author's computer only.
pub fn key_file() -> PathBuf {
    std::env::var_os("KAYZEE3D_NEWS_KEY").map(PathBuf::from).unwrap_or_else(|| crate::setup::data_folder().with_file_name("Kayzee 3D Publisher").join("news-signing.key"))
}

fn random32() -> Result<[u8; 32], String> {
    use std::io::Read;
    let mut b = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).map_err(|e| format!("no randomness: {e}"))?;
    Ok(b)
}

fn signing_key() -> Result<SigningKey, String> {
    let text = std::fs::read_to_string(key_file()).map_err(|_| format!("no signing key at {} (run --news-keygen once)", key_file().display()))?;
    let b: [u8; 32] = unhex(text.trim()).and_then(|v| v.try_into().ok()).ok_or("the signing key file is damaged")?;
    Ok(SigningKey::from_bytes(&b))
}

/// `--news-keygen`: make the private key (once) and print the public key to build into the app.
pub fn keygen() -> Result<(), String> {
    let path = key_file();
    if path.exists() {
        let k = signing_key()?;
        println!("A key already exists at {}\npublic key: {}", path.display(), hex(k.verifying_key().as_bytes()));
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let k = SigningKey::from_bytes(&random32()?);
    std::fs::write(&path, hex(&k.to_bytes())).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    println!("New private key saved at {} (keep a backup, never share it)\npublic key: {}", path.display(), hex(k.verifying_key().as_bytes()));
    Ok(())
}

/// `--news-sign FEED TITLE TEXT [LINK] [DAYS]`: add a signed message to the feed file (drops expired ones).
pub fn sign(feed_path: &Path, title: &str, body: &str, link: &str, days: u64) -> Result<(), String> {
    let key = signing_key()?;
    let now = crate::update::now_secs();
    if Some(key.verifying_key()) != public_key() {
        return Err("this signing key does not match the public key built into the app".into());
    }
    let mut m = Message { id: format!("{now}"), title: title.trim().to_owned(), body: body.trim().to_owned(), link: link.trim().to_owned(), expires: now + days.clamp(1, MAX_DAYS) * 86_400, sig: String::new() };
    m.sig = hex(&key.sign(&signed_bytes(&m)).to_bytes());
    check_message(&m, &key.verifying_key(), now)?;
    let mut feed: Feed = std::fs::read_to_string(feed_path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    feed.messages.retain(|x| x.expires > now);
    feed.messages.push(m.clone());
    std::fs::write(feed_path, serde_json::to_string_pretty(&feed).unwrap() + "\n").map_err(|e| e.to_string())?;
    println!("Signed message {} added to {} (shown until {} days from now)", m.id, feed_path.display(), days.clamp(1, MAX_DAYS));
    Ok(())
}

/// `--news-verify FEED`: what the app would do with each message.
pub fn verify_cli(feed_path: &Path) -> Result<(), String> {
    let key = public_key().ok_or("no public key built in")?;
    let text = std::fs::read_to_string(feed_path).map_err(|e| e.to_string())?;
    let feed: Feed = serde_json::from_str(&text).map_err(|e| format!("not a feed: {e}"))?;
    let now = crate::update::now_secs();
    for m in &feed.messages {
        match check_message(m, &key, now) {
            Ok(()) => println!("OK      {}  {}", m.id, m.title),
            Err(e) => println!("REFUSED {}  {}  ({e})", m.id, m.title),
        }
    }
    Ok(())
}
