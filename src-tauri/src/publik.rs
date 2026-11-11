//! publik: Sign in with publik, usage counting, and publik API as one provider option.
//!
//! The first part of this file is the publik module from
//! https://publikhq.com/sdk/publik.rs.md. GSpot changes five lines of it:
//! 1. `APP_TOKEN` is compiled in from `PUBLIK_APP_TOKEN` at build time (scripts/build-macos.sh,
//!    the CI secret), as every GSpot build before it. There is no token file in the repo.
//! 2. `APP_VERSION` is the Cargo package version.
//! 3. `curl` runs without a console window on Windows.
//! 4. The origin is https://publikhq.com.
//! 5. `start_telemetry` makes the usage-count ID file (`telemetry.json`) on the first event that
//!    goes out, so an opt-out run (`DO_NOT_TRACK=1`) makes no file, as the README promises.
//!
//! The second part ("GSpot") is what the app adds on top: the status card the AI panel
//! renders, the 402 mapping llm.rs uses, the plagiarism search, and the link between the
//! gate's choice and Settings → AI. The gate window itself is publik_gate.rs.

// publik.rs — Sign in with publik, usage counting, and publik API as one provider option.
// At app start, before any window of your own: `let gate = publik::ensure_signed_in(&mut ui);`. It
//   1. loads or provisions this computer's publik key (POST /api/v1/installs);
//   2. shows Sign in with publik until GET /api/v1/wallet says "claimed" (no other way past it);
//   3. when USES_AI: asks once how the app pays for AI and stores the answer (set_choice() changes it);
//   4. starts usage counting: app_opened now, active_session every 30 minutes (POST /api/v1/telemetry).
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const APP_SLUG: &str = "gspot";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// false: the app calls no model. It still signs in and counts users; it skips the plan screen.
pub const USES_AI: bool = true;
/// Ask for a tier, never a vendor model name.
pub const MODELS: [&str; 3] = ["publik-fast", "publik-balanced", "publik-smart"];
// Public by design: it can only mint capped keys attributed to this app. GSpot compiles it in
// from PUBLIK_APP_TOKEN at build time (scripts/build-macos.sh, the CI secret). A build without
// it has no publik option: the gate says so, and your own key keeps working.
const APP_TOKEN: &str = match option_env!("PUBLIK_APP_TOKEN") {
    Some(t) => t,
    None => "",
};
const OS: &str = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "linux" };

fn origin() -> String { std::env::var("PUBLIK_ORIGIN").unwrap_or_else(|_| "https://publikhq.com".into()) } // PUBLIK_ORIGIN: tests only
pub fn default_base_url() -> String { format!("{}/api/v1", origin()) }

pub enum Provider { Own { base_url: String, api_key: String }, Publik { base_url: String, api_key: String }, NeedsOwnKey }
pub struct Screen { pub id: &'static str, pub title: &'static str, pub body: String, pub note: Option<&'static str>, pub buttons: Vec<(&'static str, &'static str)> }
/// show(): draw the screen and return the button id pressed. Once `done` is set (the gate saw the sign-in), close it and return None.
pub trait GateUi { fn show(&mut self, screen: &Screen, done: Arc<AtomicBool>) -> Option<&'static str>; fn open_url(&mut self, url: &str); }

/// ~/Library/Application Support/publik, %LOCALAPPDATA%\publik, ~/.config/publik: shared by every publik app.
fn publik_dir() -> PathBuf {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    let base = if cfg!(target_os = "macos") { home.join("Library/Application Support") }
        else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or(home.join("AppData/Local")) }
        else { std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home.join(".config")) };
    base.join("publik")
}
pub fn credential_path() -> PathBuf { publik_dir().join("apps").join(format!("{APP_SLUG}.json")) }

pub fn read_credential() -> Option<Value> {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(credential_path()).ok()?).ok()?;
    v["key"].as_str().filter(|k| k.starts_with("pk_"))?;
    Some(v)
}
fn write_credential(v: &Value) -> Result<(), String> {
    let path = credential_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, serde_json::to_string_pretty(v).unwrap()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)); }
    Ok(())
}
/// The person's answer to the plan screen: Some("publik"), Some("own_key"), or None to ask again. Settings call it.
pub fn set_choice(choice: Option<&str>) { if let Some(mut c) = read_credential() { c["choice"] = json!(choice); let _ = write_credential(&c); } }

/// The key this app uses: the stored choice decides. PUBLIK_API_KEY / PUBLIK_API_BASE_URL override (development).
pub fn resolve_provider(user_key: Option<(String, String)>) -> Provider {
    if let Ok(key) = std::env::var("PUBLIK_API_KEY") {
        return Provider::Publik { base_url: std::env::var("PUBLIK_API_BASE_URL").unwrap_or_else(|_| default_base_url()), api_key: key };
    }
    if let Some(c) = read_credential().filter(|c| c["choice"] == "publik") {
        return Provider::Publik { base_url: c["base_url"].as_str().map(String::from).unwrap_or_else(default_base_url), api_key: c["key"].as_str().unwrap_or_default().into() };
    }
    match user_key.filter(|(_, k)| !k.is_empty()) { Some((base_url, api_key)) => Provider::Own { base_url, api_key }, None => Provider::NeedsOwnKey }
}

/// One curl call; `stdin` carries the body or a config with the key. Err: publik is not reachable.
fn curl(args: &[&str], stdin: &str) -> Result<(u16, Value), String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-w", "\n%{http_code}"]).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000); } // CREATE_NO_WINDOW: no console flashes up
    let mut child = cmd.spawn().map_err(|e| format!("curl: {e}"))?;
    child.stdin.take().unwrap().write_all(stdin.as_bytes()).map_err(|e| e.to_string())?;
    let out = String::from_utf8_lossy(&child.wait_with_output().map_err(|e| e.to_string())?.stdout).to_string();
    let (payload, code) = out.rsplit_once('\n').unwrap_or(("", &out));
    match code.trim().parse::<u16>().unwrap_or(0) {
        0 => Err("publik is not reachable. Check your connection.".into()),
        code => Ok((code, serde_json::from_str(payload).unwrap_or(json!({})))),
    }
}
fn key_config(key: &str) -> String { format!("header = \"Authorization: Bearer {key}\"\n") }

/// POST /api/v1/installs: this computer's publik key. Err is a readable message.
pub fn provision() -> Result<Value, String> {
    let runtime = std::env::var("PUBLIK_APP_TOKEN").unwrap_or_default();
    let token = if runtime.trim().is_empty() { APP_TOKEN.trim() } else { runtime.trim() };
    if !token.starts_with("pat_") { return Err("This build has no publik app token. Ask the developer for a new build.".into()); }
    let mut prior = read_credential().unwrap_or(json!({}));
    let mut install_id = prior["install_id"].as_str().map(String::from).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    for _ in 0..2 {
        let body = json!({ "app_token": token, "app_slug": APP_SLUG, "app_version": APP_VERSION, "os": OS, "arch": std::env::consts::ARCH,
            "install_id": install_id, "disclosure_version": 1, "dialects": ["chat_completions"] });
        let url = format!("{}/api/v1/installs", origin());
        let (code, res) = curl(&["-m", "20", "-X", "POST", &url, "-H", "content-type: application/json", "--data-binary", "@-"], &body.to_string())?;
        let key = res["key"].as_str().or(if code == 200 { prior["key"].as_str() } else { None }).map(String::from);
        if let (200 | 201, Some(key)) = (code, key) {
            let c = json!({ "key": key, "base_url": res["base_url"].as_str().map(String::from).unwrap_or_else(default_base_url),
                "install_id": res["install_id"].as_str().unwrap_or(&install_id), "claim_url": res["claim_url"], "choice": prior["choice"] });
            write_credential(&c)?; // the key reaches disk before anything else sees it
            return Ok(c);
        }
        if code == 200 || res["error"]["type"] == "install_revoked" { install_id = uuid::Uuid::new_v4().to_string(); prior = json!({}); continue; }
        return Err(res["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("publik could not set up this computer ({code}).")));
    }
    Err("publik could not set up this computer.".into())
}

/// GET /api/v1/wallet: (status, wallet or None, message). Err: publik is not reachable.
pub fn read_wallet(key: &str) -> Result<(u16, Option<Value>, Option<String>), String> {
    let url = format!("{}/api/v1/wallet", origin());
    let (code, mut b) = curl(&["-m", "15", "-K", "-", &url], &key_config(key))?;
    if code != 200 || !b["claim_state"].is_string() { return Ok((code, None, b["error"]["message"].as_str().map(String::from))); }
    if !b["plan"].is_object() { b["plan"] = json!({ "kind": "none", "label": null }); }
    Ok((code, Some(b), None))
}

/// The words of the gate. The same in every publik module.
pub fn gate_screen(id: &'static str, w: Option<&Value>, note: Option<&'static str>, message: Option<String>) -> Screen {
    if id == "unreachable" {
        return Screen { id, title: "Sign in with publik", body: message.unwrap_or_else(|| "publik is not reachable. Check your connection.".into()), note, buttons: vec![("retry", "Retry")] };
    }
    if id == "sign_in" {
        return Screen { id, title: "Sign in with publik", note, buttons: vec![("sign_in", "Sign in with publik")],
            body: "This app uses your publik account. Click Sign in with publik, then sign in in your browser. This window continues when you are signed in.".into() };
    }
    let w = w.cloned().unwrap_or(json!({ "plan": { "kind": "none" } }));
    let who = match w["account"]["handle"].as_str() { Some(h) => format!("You are signed in as @{h}."), None => "You are signed in to publik.".into() };
    if !has_plan(&w) {
        return Screen { id, title: "Choose how to pay for AI", note, body: format!("{who} This app uses AI. Get a publik plan, or use your own key."),
            buttons: vec![("get_plan", "Get a plan"), ("own_key", "Use my own key")] };
    }
    let what = if w["plan"]["kind"] == "plan" { format!("Your plan: {}.", w["plan"]["label"].as_str().unwrap_or("publik")) }
        else { format!("Your balance: ${:.2}.", w["balance_micros"].as_i64().unwrap_or(0) as f64 / 1_000_000.0) };
    Screen { id, title: "Connect your plan", note, body: format!("{who} {what} Connect it to use AI in this app, or use your own key."),
        buttons: vec![("connect", "Connect your plan"), ("own_key", "Use my own key")] }
}
fn claimed(w: &Value) -> bool { w["claim_state"] == "claimed" }
fn has_plan(w: &Value) -> bool { w["plan"]["kind"].as_str().unwrap_or("none") != "none" || w["balance_micros"].as_i64().unwrap_or(0) > 0 }

/// Show a screen; with `until`, poll GET /wallet every 3 s for at most 10 minutes. "done": the poll saw it; "timeout": it did not.
fn ask(ui: &mut dyn GateUi, screen: &Screen, key: &str, until: Option<fn(&Value) -> bool>) -> &'static str {
    let done = Arc::new(AtomicBool::new(false));
    let polled: Arc<Mutex<Option<&'static str>>> = Arc::new(Mutex::new(None));
    if let Some(until) = until {
        let (done, polled, key) = (done.clone(), polled.clone(), key.to_string());
        std::thread::spawn(move || {
            let mut result = "timeout";
            'poll: for _ in 0..200 { // 200 x 3 s = 10 minutes
                for _ in 0..30 { std::thread::sleep(Duration::from_millis(100)); if done.load(Ordering::SeqCst) { return; } }
                if let Ok((_, Some(w), _)) = read_wallet(&key) { if until(&w) { result = "done"; break 'poll; } }
            }
            *polled.lock().unwrap() = Some(result);
            done.store(true, Ordering::SeqCst);
        });
    }
    let action = ui.show(screen, done.clone());
    done.store(true, Ordering::SeqCst);
    let polled = *polled.lock().unwrap();
    action.or(polled).unwrap_or("retry")
}

/// The gate (see the top of the file). Returns (key, wallet, choice) once the person is signed in; choice is None when USES_AI is false.
pub fn ensure_signed_in(ui: &mut dyn GateUi) -> (String, Value, Option<String>) {
    if std::env::var_os("PUBLIK_SELF_TEST").is_some() { println!("{}", self_test()); std::process::exit(0); }
    let (mut sign_note, mut sign_poll, mut plan_note, mut plan_poll): (Option<&'static str>, bool, Option<&'static str>, bool) = (None, true, None, false);
    loop {
        let env_key = std::env::var("PUBLIK_API_KEY").ok().filter(|k| !k.is_empty());
        let c = match env_key.clone().map(|k| Ok(json!({ "key": k }))).or_else(|| read_credential().map(Ok)).unwrap_or_else(provision) {
            Ok(c) => c,
            Err(e) => { ask(ui, &gate_screen("unreachable", None, None, Some(e)), "", None); continue; }
        };
        let key = c["key"].as_str().unwrap_or_default().to_string();
        let w = match read_wallet(&key) {
            Err(_) => { ask(ui, &gate_screen("unreachable", None, None, None), "", None); continue; }
            Ok((401, _, _)) if env_key.is_none() => { let _ = write_credential(&json!({})); continue; } // a revoked key: provision again
            Ok((_, None, message)) => { ask(ui, &gate_screen("unreachable", None, None, message), "", None); continue; }
            Ok((_, Some(w), _)) => w,
        };
        if !claimed(&w) {
            let a = ask(ui, &gate_screen("sign_in", Some(&w), sign_note, None), &key, if sign_poll { Some(claimed) } else { None });
            if a == "sign_in" {
                ui.open_url(w["claim_url"].as_str().or(c["claim_url"].as_str()).unwrap_or(&format!("{}/wallet", origin())));
                (sign_note, sign_poll) = (Some("Waiting for you to sign in in your browser."), true);
            }
            if a == "timeout" { (sign_note, sign_poll) = (Some("No sign-in after 10 minutes. Click Sign in with publik to try again."), false); }
            continue;
        }
        let mut choice = if USES_AI { read_credential().and_then(|c| c["choice"].as_str().map(String::from)) } else { None };
        if USES_AI && choice.is_none() {
            if has_plan(&w) { (plan_note, plan_poll) = (None, false); } // the plan is there: Connect your plan
            let a = ask(ui, &gate_screen("choose", Some(&w), plan_note, None), &key, if plan_poll { Some(has_plan) } else { None });
            (plan_note, plan_poll) = (None, false);
            if a == "get_plan" {
                ui.open_url(w["top_up_url"].as_str().unwrap_or(&format!("{}/wallet", origin())));
                (plan_note, plan_poll) = (Some("Waiting for your plan. Finish in your browser."), true);
            }
            if a == "timeout" { plan_note = Some("No plan after 10 minutes. Click Get a plan to try again."); }
            if a != "connect" && a != "own_key" { continue; } // "done": the plan is there; the next screen is Connect your plan
            choice = Some(if a == "connect" { "publik" } else { "own_key" }.to_string());
            set_choice(choice.as_deref());
        }
        start_telemetry();
        return (key, w, choice);
    }
}

/// Builder check, run by the BUILT app: PUBLIK_SELF_TEST=1 makes ensure_signed_in() print this line and exit.
pub fn self_test() -> String {
    let result = provision().and_then(|c| read_wallet(c["key"].as_str().unwrap_or_default())).and_then(|(code, w, message)| match w {
        Some(_) => Ok(()),
        None => Err(message.unwrap_or_else(|| format!("GET /wallet answered {code}"))),
    });
    match result { Ok(()) => format!("publik self-test: ok {APP_SLUG} {APP_VERSION}"), Err(e) => format!("publik self-test: failed: {e}") }
}

/// A terminal UI for a command-line app. A desktop app passes its own GateUi (see the page).
pub struct TerminalUi;
impl GateUi for TerminalUi {
    fn show(&mut self, s: &Screen, done: Arc<AtomicBool>) -> Option<&'static str> {
        println!("\n{}\n{}{}", s.title, s.body, s.note.map(|n| format!("\n{n}")).unwrap_or_default());
        for (i, (_, label)) in s.buttons.iter().enumerate() { println!("  {}. {label}", i + 1); }
        let answered = Arc::new(AtomicBool::new(false));
        let (d, a) = (done.clone(), answered.clone());
        std::thread::spawn(move || { while !d.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(200)); } if !a.load(Ordering::SeqCst) { println!("\nDone. Press Enter to continue."); } });
        loop {
            print!("{}", if s.buttons.len() > 1 { "Type a number and press Enter: " } else { "Press Enter: " });
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 { while !done.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(200)); } return None; }
            if done.load(Ordering::SeqCst) { return None; }
            let n: usize = line.trim().parse().unwrap_or(1);
            if let Some((action, _)) = s.buttons.get(n.wrapping_sub(1)) { answered.store(true, Ordering::SeqCst); return Some(action); }
        }
    }
    fn open_url(&mut self, url: &str) {
        println!("Open this link if your browser does not open: {url}");
        let _ = if cfg!(target_os = "macos") { Command::new("open").arg(url).spawn() }
            else if cfg!(windows) { Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn() }
            else { Command::new("xdg-open").arg(url).spawn() };
    }
}

// ---------- usage counting: /skills/telemetry.md sections 3-5 ----------
static TELEMETRY_STARTED: AtomicBool = AtomicBool::new(false);

fn usage_off() -> bool {
    let env = |n: &str| std::env::var(n).unwrap_or_default().trim().to_lowercase();
    if !["", "0", "false"].contains(&env("DO_NOT_TRACK").as_str()) || ["0", "false", "off"].contains(&env("PUBLIK_TELEMETRY").as_str()) { return true; }
    std::fs::read_to_string(publik_dir().join("consent.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).map(|v| v["usage"] == false).unwrap_or(false)
}
fn is_uuid_v4(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36 && b.iter().enumerate().all(|(i, c)| if [8, 13, 18, 23].contains(&i) { *c == b'-' } else { c.is_ascii_hexdigit() })
        && b[14] == b'4' && b"89abAB".contains(&b[19])
}
fn utc_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64 + 719_468, secs % 86_400); // days since 0000-03-01
    let (era, doe) = (days / 146_097, days % 146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let (day, month) = (doy - (153 * mp + 2) / 5 + 1, if mp < 10 { mp + 3 } else { mp - 9 });
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", yoe + era * 400 + i64::from(month <= 2), month, day, rem / 3600, rem % 3600 / 60, rem % 60)
}
/// One random UUID per computer account, shared by every publik app, in <publik dir>/telemetry.json (mode 0600).
fn telemetry_install_id() -> String {
    let file = publik_dir().join("telemetry.json");
    let saved = || std::fs::read_to_string(&file).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v["install_id"].as_str().map(str::to_lowercase)).filter(|id| is_uuid_v4(id));
    if let Some(id) = saved() { return id; }
    let fresh = uuid::Uuid::new_v4().to_string(); // if the file cannot be saved, this ID lasts for this run only
    let _ = std::fs::create_dir_all(publik_dir());
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true); // never over another app's file
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    match options.open(&file) {
        Ok(mut f) => { let _ = writeln!(f, "{}", json!({ "install_id": fresh, "created_at": utc_now() })); fresh }
        Err(_) => saved().unwrap_or(fresh), // another publik app saved its ID first
    }
}
fn send_event(event: &str, install_id: &str) {
    let say = |result: &str| if std::env::var("PUBLIK_TELEMETRY_DEBUG").as_deref() == Ok("1") { eprintln!("publik-telemetry: {event} -> {result}"); };
    if usage_off() { return say("off (opt-out)"); }
    let key = std::env::var("PUBLIK_API_KEY").ok().or_else(|| read_credential().and_then(|c| c["key"].as_str().map(String::from))).filter(|k| k.starts_with("pk_"));
    let mut body = json!({ "v": 1, "app": APP_SLUG, "install_id": install_id, "event": event, "app_version": APP_VERSION.trim_start_matches(['v', 'V']), "os": OS });
    match std::env::consts::ARCH { "aarch64" => body["arch"] = json!("arm64"), "x86_64" => body["arch"] = json!("x64"), _ => {} }
    let url = std::env::var("PUBLIK_TELEMETRY_URL").unwrap_or_else(|_| format!("{}/api/v1/telemetry", origin()));
    let body = body.to_string();
    let args = ["-m", "5", "-K", "-", "-X", "POST", url.as_str(), "-H", "content-type: application/json", "--data-raw", body.as_str()];
    match curl(&args, &key.map(|k| key_config(&k)).unwrap_or_default()) { Ok((code, _)) => say(&code.to_string()), Err(e) => say(&e) }
}
/// Called by ensure_signed_in(). app_opened now, active_session every 30 minutes, on a background thread; never panics the app.
pub fn start_telemetry() {
    if TELEMETRY_STARTED.swap(true, Ordering::SeqCst) { return; }
    let s: u64 = std::env::var("PUBLIK_TELEMETRY_INTERVAL_SECONDS").ok().and_then(|v| v.parse().ok()).unwrap_or(0); // tests only
    let interval = Duration::from_secs(if (1..=86_400).contains(&s) { s } else { 1_800 });
    std::thread::spawn(move || {
        let mut id = String::new(); // made on the first event that goes out: an opt-out run makes no ID file
        let mut event = "app_opened";
        loop {
            if !usage_off() && id.is_empty() {
                id = telemetry_install_id();
            }
            send_event(event, &id);
            std::thread::sleep(interval);
            event = "active_session";
        }
    });
}

// ---------- publik API calls ----------
/// A 402 renders its message plus exactly ONE link: this one.
pub fn top_up_url_from_402(body: &Value) -> (String, Option<String>) {
    let e = &body["error"];
    (e["message"].as_str().unwrap_or("Not enough publik API balance.").into(), e["top_up_url"].as_str().map(String::from))
}

/// `POST /search`: web results for `q` (quote a phrase to find it exactly), one published price per search; Err = (message, top_up_url) from a 402 or a readable failure.
pub fn search_results(base_url: &str, api_key: &str, q: &str, count: u8) -> Result<Vec<Value>, (String, Option<String>)> {
    let (url, body) = (format!("{base_url}/search"), json!({ "q": q, "count": count }).to_string());
    match curl(&["-m", "30", "-K", "-", "-X", "POST", &url, "-H", "content-type: application/json", "--data-raw", &body], &key_config(api_key)) {
        Ok((200, v)) => Ok(v["results"].as_array().cloned().unwrap_or_default()),
        Ok((402, v)) => Err(top_up_url_from_402(&v)),
        Ok((c, _)) => Err((format!("publik API answered {c}."), None)),
        Err(e) => Err((e, None)),
    }
}

// ============================================================================ GSpot
// What GSpot adds to the module: the status card the AI panel renders, the 402 mapping
// llm.rs uses, the plagiarism search, and the link between the gate's choice and
// Settings → AI. Copy rule: "publik API", dollars, never tokens, never the provider's name.

use serde::Serialize;

pub const DEFAULT_BASE_URL: &str = "https://publikhq.com/api/v1";
pub const DASHBOARD_URL: &str = "https://publikhq.com/dashboard/api";
pub const TERMS_URL: &str = "https://publikhq.com/terms#api";
pub const DISCLOSURE_VERSION: u32 = 1;
/// The tier the sidebar features use: the writing quality they need. A tier, never a vendor model name.
pub const DEFAULT_MODEL: &str = "publik-balanced";
/// The only host a claim / top-up link may point at. Anything else is dropped.
const LINK_HOST_PREFIX: &str = "https://publikhq.com/";

/// The one-sentence justification, shown wherever money is mentioned.
pub const WHY_IT_COSTS: &str = "The model's provider charges per use. publik charges a fixed, published price that sits above what the model costs publik; the difference keeps publik running and pays the developer who built this app. Nothing is charged behind your back, and every call is visible on your publik dashboard.";
/// The AI picker's cost sentence.
pub const DISCLOSURE_COST: &str = "Every request is priced per use at a fixed, published price from your publik plan or balance, and every charge is visible on your dashboard.";
pub const DISCLOSURE_DATA: &str = "The text you check goes through publik's servers to a shared model account. publik never trains on it and does not store it. Spelling and grammar checks stay on this computer.";
pub const CTA_SIGN_IN: &str = "Sign in with publik";
pub const CTA_ADD_PLAN: &str = "Add a plan or pack";
pub const CTA_LATER: &str = "Later";

/// This computer's publik key, as llm.rs and the plagiarism search use it.
#[derive(Clone, Debug, Default)]
pub struct Credential {
    pub key: String,
    pub base_url: String,
    pub install_id: String,
    pub claim_url: String,
    pub model: String,
}

/// The key in force: `PUBLIK_API_KEY` in the environment (development), else the credential file the gate wrote.
pub fn credential() -> Option<Credential> {
    if let Ok(k) = std::env::var("PUBLIK_API_KEY") {
        let k = k.trim().to_string();
        if k.starts_with("pk_") {
            return Some(Credential { key: k, base_url: resolve_base_url(""), model: DEFAULT_MODEL.into(), ..Default::default() });
        }
    }
    let c = read_credential()?;
    Some(Credential {
        key: c["key"].as_str().unwrap_or_default().to_string(),
        base_url: resolve_base_url(c["base_url"].as_str().unwrap_or_default()),
        install_id: c["install_id"].as_str().unwrap_or_default().to_string(),
        claim_url: publik_link(c["claim_url"].as_str()).unwrap_or_default(),
        model: DEFAULT_MODEL.into(),
    })
}

/// Forget this computer's key. The next start signs in again.
pub fn forget_credential() {
    let _ = std::fs::remove_file(credential_path());
    *WALLET.lock().unwrap() = None;
    clear_live();
}

pub fn publik_link(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    if s.starts_with(LINK_HOST_PREFIX) {
        Some(s.to_string())
    } else {
        None
    }
}

fn trim_url(s: &str) -> Option<String> {
    let t = s.trim().trim_end_matches('/');
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// `PUBLIK_API_BASE_URL` (dev) → the credential's `base_url` → the default.
pub fn resolve_base_url(from_credential: &str) -> String {
    if let Some(u) = std::env::var("PUBLIK_API_BASE_URL").ok().and_then(|s| trim_url(&s)) {
        return u;
    }
    trim_url(from_credential).unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
}

/// A typed gateway failure: what to show, and the one link to render.
#[derive(Clone, Debug, Serialize)]
pub struct PublikError {
    pub status: u16,
    pub kind: String,
    pub message: String,
    pub link_label: String,
    pub link_url: Option<String>,
    pub retry_after_secs: Option<u64>,
    pub reprovision: bool,
}

impl std::fmt::Display for PublikError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for PublikError {}

/// Live state from the last gateway answer (headers, 402s, transport errors).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Live {
    pub balance_micros: Option<i64>,
    pub last_charge_micros: Option<i64>,
    pub claim_state: Option<String>,
    pub week_used_micros: Option<i64>,
    pub week_budget_micros: Option<i64>,
    pub model_served: Option<String>,
    pub exhausted: bool,
    pub disconnected: bool,
    pub unreachable: bool,
    pub notice: Option<Notice>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Notice {
    pub kind: String,
    pub message: String,
    pub link_label: String,
    pub link_url: String,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
/// The wallet the gate read at start-up (and every refresh since): the card's plan, balance and links.
static WALLET: Mutex<Option<Value>> = Mutex::new(None);

fn live() -> Live {
    LIVE.lock().unwrap().clone().unwrap_or_default()
}
fn with_live(f: impl FnOnce(&mut Live)) {
    let mut g = LIVE.lock().unwrap();
    let mut l = g.clone().unwrap_or_default();
    f(&mut l);
    *g = Some(l);
}
pub fn clear_live() {
    *LIVE.lock().unwrap() = None;
}
pub fn dismiss_notice() {
    with_live(|l| l.notice = None);
}

fn header_str(h: &reqwest::header::HeaderMap, k: &str) -> Option<String> {
    h.get(k).and_then(|v| v.to_str().ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn header_i64(h: &reqwest::header::HeaderMap, k: &str) -> Option<i64> {
    header_str(h, k).and_then(|s| s.parse::<i64>().ok())
}

fn err_field<'a>(v: &'a Value, key: &str) -> &'a Value {
    let nested = &v["error"][key];
    if nested.is_null() { &v[key] } else { nested }
}

/// Map a non-2xx gateway answer to what the UI shows.
pub fn map_error(status: u16, headers: &reqwest::header::HeaderMap, body: &str, claim_url_fallback: Option<&str>) -> PublikError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let kind = err_field(&v, "type").as_str().unwrap_or("").to_string();
    let retry = header_str(headers, "retry-after").and_then(|s| s.parse::<u64>().ok());
    match status {
        402 => {
            let claim_state = err_field(&v, "claim_state").as_str().unwrap_or("anonymous").to_string();
            let link = publik_link(err_field(&v, "top_up_url").as_str())
                .or_else(|| publik_link(err_field(&v, "claim_url").as_str()))
                .or_else(|| publik_link(claim_url_fallback))
                .or_else(|| Some(DASHBOARD_URL.to_string()));
            PublikError {
                status,
                kind: if kind.is_empty() { "insufficient_credit".into() } else { kind },
                message: err_field(&v, "message").as_str().filter(|m| !m.trim().is_empty()).unwrap_or("Not enough publik balance for this request.").to_string(),
                link_label: if claim_state == "claimed" { CTA_ADD_PLAN.into() } else { CTA_SIGN_IN.into() },
                link_url: link,
                retry_after_secs: None,
                reprovision: false,
            }
        }
        401 | 403 if kind == "key_revoked" => PublikError {
            status,
            kind,
            message: "This computer was removed from your publik account. Quit and open GSpot again to sign in.".into(),
            link_label: "Open the publik dashboard".into(),
            link_url: Some(DASHBOARD_URL.into()),
            retry_after_secs: None,
            reprovision: err_field(&v, "reprovision").as_bool().unwrap_or(false),
        },
        401 => PublikError {
            status,
            kind: "invalid_api_key".into(),
            message: "publik API did not accept this computer's key. Quit and open GSpot again to sign in.".into(),
            link_label: String::new(),
            link_url: None,
            retry_after_secs: None,
            reprovision: false,
        },
        429 => PublikError {
            status,
            kind: if kind.is_empty() { "rate_limit_exceeded".into() } else { kind },
            message: err_field(&v, "message").as_str().unwrap_or("publik API asked us to slow down. Try again in a moment.").to_string(),
            link_label: String::new(),
            link_url: None,
            retry_after_secs: retry,
            reprovision: false,
        },
        503 => PublikError {
            status,
            kind: "gateway_unavailable".into(),
            message: "publik API is unavailable right now. Nothing was charged. Try again in a minute.".into(),
            link_label: String::new(),
            link_url: None,
            retry_after_secs: retry,
            reprovision: false,
        },
        _ => PublikError {
            status,
            kind: if kind.is_empty() { format!("http_{status}") } else { kind },
            message: err_field(&v, "message").as_str().map(String::from).unwrap_or_else(|| format!("publik API answered {status}.")),
            link_label: String::new(),
            link_url: None,
            retry_after_secs: None,
            reprovision: false,
        },
    }
}

/// Record the `x-publik-*` headers of a successful metered call.
pub fn note_response(h: &reqwest::header::HeaderMap) {
    let Some(balance) = header_i64(h, "x-publik-balance").or_else(|| header_i64(h, "x-publik-balance-micros")) else { return };
    with_live(|l| {
        l.balance_micros = Some(balance);
        l.last_charge_micros = header_i64(h, "x-publik-charge-micros").or(l.last_charge_micros);
        l.claim_state = header_str(h, "x-publik-claim-state").or(l.claim_state.take());
        l.week_used_micros = header_i64(h, "x-publik-week-used");
        l.week_budget_micros = header_i64(h, "x-publik-week-budget");
        l.model_served = header_str(h, "x-publik-model");
        l.exhausted = false;
        l.unreachable = false;
        l.disconnected = false;
        if l.notice.as_ref().map(|n| n.kind == "exhausted" || n.kind == "unreachable").unwrap_or(false) {
            l.notice = None;
        }
    });
}

/// Record a gateway failure so the status card can explain it.
pub fn note_error(e: &PublikError) {
    with_live(|l| {
        l.unreachable = e.status == 503 || e.status == 0;
        l.exhausted = e.status == 402;
        l.disconnected = e.kind == "key_revoked" || e.kind == "invalid_api_key";
        l.notice = Some(Notice {
            kind: match e.status { 402 => "exhausted", 503 | 0 => "unreachable", _ => "error" }.into(),
            message: e.message.clone(),
            link_label: e.link_label.clone(),
            link_url: e.link_url.clone().unwrap_or_default(),
        });
    });
}

pub fn note_transport_error(detail: &str) {
    with_live(|l| {
        l.unreachable = true;
        l.notice = Some(Notice {
            kind: "unreachable".into(),
            message: format!("publik API is unreachable right now ({detail}). Nothing was charged."),
            link_label: String::new(),
            link_url: String::new(),
        });
    });
}

/// Keep a wallet answer (the gate's at start-up, a refresh since) for the card.
pub fn remember_wallet(w: &Value) {
    *WALLET.lock().unwrap() = Some(w.clone());
    with_live(|l| {
        l.balance_micros = w["balance_micros"].as_i64();
        l.claim_state = w["claim_state"].as_str().map(String::from);
        l.week_used_micros = w["week"]["used_micros"].as_i64();
        l.week_budget_micros = w["week"]["budget_micros"].as_i64();
        l.unreachable = false;
        l.disconnected = false;
        if has_plan(w) {
            l.exhausted = false;
            if l.notice.as_ref().map(|n| n.kind == "exhausted" || n.kind == "unreachable").unwrap_or(false) {
                l.notice = None;
            }
        }
    });
}

/// `GET /wallet` now (blocking): the card's source of truth when there is no fresh header.
pub fn refresh_wallet() {
    let Some(c) = credential() else { return };
    match read_wallet(&c.key) {
        Ok((_, Some(w), _)) => remember_wallet(&w),
        Ok((code, None, message)) => note_error(&map_error(code, &reqwest::header::HeaderMap::new(), &json!({ "error": { "message": message } }).to_string(), Some(&c.claim_url))),
        Err(e) => note_transport_error(&e),
    }
}

/// One web search through publik API (`POST /search`): links and titles for `q`, at
/// publik's published price per search. A quoted `q` finds the exact phrase, which is
/// what the plagiarism check sends.
pub async fn search(cred: &Credential, q: &str, count: u8) -> Result<Vec<(String, String)>, PublikError> {
    let (base_url, key, q) = (cred.base_url.clone(), cred.key.clone(), q.to_string());
    let results = tauri::async_runtime::spawn_blocking(move || search_results(&base_url, &key, &q, count)).await.map_err(|e| transport(&e.to_string()))?;
    match results {
        Ok(items) => Ok(items.iter().filter_map(|r| Some((r["url"].as_str()?.to_string(), r["title"].as_str().unwrap_or("").to_string()))).collect()),
        Err((message, top_up_url)) => {
            let status = if top_up_url.is_some() || message.starts_with("Not enough") { 402 } else if message.contains("not reachable") { 0 } else { 500 };
            let e = PublikError {
                status,
                kind: if status == 402 { "insufficient_credit".into() } else if status == 0 { "transport".into() } else { "http".into() },
                message,
                link_label: if status == 402 { CTA_ADD_PLAN.into() } else { String::new() },
                link_url: if status == 402 { publik_link(top_up_url.as_deref()).or_else(|| Some(DASHBOARD_URL.into())) } else { None },
                retry_after_secs: None,
                reprovision: false,
            };
            note_error(&e);
            Err(e)
        }
    }
}

fn transport(detail: &str) -> PublikError {
    PublikError { status: 0, kind: "transport".into(), message: format!("Could not reach publik ({detail}). Check your internet connection and try again."), link_label: String::new(), link_url: None, retry_after_secs: None, reprovision: false }
}

/// The credential the plagiarism check searches with: only when publik API is the
/// chosen provider. A user on their own model key keeps the keyless chain.
pub fn search_credential(settings: &crate::settings::Settings) -> Option<Credential> {
    if settings.llm.provider == crate::settings::Provider::Publik { credential() } else { None }
}

/// "$0.0004" / "$1.23".
pub fn fmt_usd(micros: i64) -> String {
    let d = micros as f64 / 1_000_000.0;
    if d.abs() < 0.01 && d != 0.0 {
        format!("${d:.4}")
    } else {
        format!("${d:.2}")
    }
}

/// The balance line: the plan when there is one, else the live balance ("$0.41 left"),
/// or "sign in with publik" while the computer is not signed in.
pub fn balance_line(balance_micros: Option<i64>, claim_state: &str, plan_label: Option<&str>) -> String {
    if let Some(label) = plan_label {
        return format!("Your plan: {label}");
    }
    let b = fmt_usd(balance_micros.unwrap_or(0));
    if claim_state == "claimed" {
        format!("{b} left")
    } else {
        format!("{b} · sign in with publik to use it")
    }
}

/// Everything the UI renders about publik API, from the credential, the wallet and the live state.
pub fn status(settings: &crate::settings::Settings) -> Value {
    let cred = credential();
    let l = live();
    let w = WALLET.lock().unwrap().clone();
    let claim_state = l.claim_state.clone().or_else(|| w.as_ref().and_then(|w| w["claim_state"].as_str().map(String::from))).filter(|s| !s.is_empty()).unwrap_or_else(|| "anonymous".into());
    let claimed = claim_state == "claimed";
    let plan_label = w.as_ref().filter(|w| w["plan"]["kind"] == "plan").map(|w| w["plan"]["label"].as_str().unwrap_or("publik").to_string());
    let balance = l.balance_micros.or_else(|| w.as_ref().and_then(|w| w["balance_micros"].as_i64()));
    let top_up_url = w.as_ref().and_then(|w| publik_link(w["top_up_url"].as_str())).unwrap_or_else(|| "https://publikhq.com/wallet".into());
    let claim_url = cred.as_ref().map(|c| c.claim_url.clone()).filter(|u| !u.is_empty()).unwrap_or_else(|| DASHBOARD_URL.into());
    let cta_url = if claimed { top_up_url } else { claim_url };
    let selected = settings.llm.provider == crate::settings::Provider::Publik;
    let state = match (&cred, l.disconnected, l.exhausted, l.unreachable) {
        (None, ..) => "not_set_up",
        (_, true, _, _) => "disconnected",
        (_, _, true, _) => "needs_credit",
        (_, _, _, true) => "unreachable",
        _ => "ready",
    };
    json!({
        "available": !APP_TOKEN.trim().is_empty() || cred.is_some(),
        "can_provision": !APP_TOKEN.trim().is_empty(),
        "has_key": cred.is_some(),
        "selected": selected,
        "state": state,
        "claim_state": claim_state,
        "balance_micros": balance,
        "balance_line": balance_line(balance, &claim_state, plan_label.as_deref()),
        "last_charge_label": l.last_charge_micros.map(|m| format!("last request {}", fmt_usd(m))),
        "week_label": match (l.week_used_micros, l.week_budget_micros) { (Some(u), Some(b)) if b > 0 => Some(format!("This week {} of {}", fmt_usd(u), fmt_usd(b))), _ => None },
        "model": cred.as_ref().map(|c| c.model.clone()).unwrap_or_else(|| DEFAULT_MODEL.into()),
        "model_served": l.model_served,
        "justification": WHY_IT_COSTS,
        "disclosure": {"cost": DISCLOSURE_COST, "data": DISCLOSURE_DATA, "terms_url": TERMS_URL},
        "cta": {"label": if claimed { CTA_ADD_PLAN } else { CTA_SIGN_IN }, "url": cta_url, "claimed": claimed},
        "later_label": CTA_LATER,
        "first_run_owed": settings.publik.first_run_owed && cred.is_some(),
        "dashboard_url": DASHBOARD_URL,
        "notice": l.notice,
    })
}

/// The gate's answer, applied to Settings → AI once at start-up: "publik" selects publik API;
/// "own_key" keeps an own-key provider as it is, and moves the publik default to the own-key
/// card, which then asks for the key.
pub fn apply_choice(settings: &mut crate::settings::Settings, choice: Option<&str>) {
    let publik = crate::settings::Provider::Publik;
    match choice {
        Some("publik") => {
            if settings.llm.provider != publik {
                settings.llm = crate::settings::LlmConfig::default();
            }
        }
        Some("own_key") => {
            if settings.llm.provider == publik {
                settings.llm = crate::settings::LlmConfig { provider: crate::settings::Provider::OpenAI, base_url: "https://api.openai.com".into(), api_key: String::new(), model: "gpt-4o-mini".into() };
            }
        }
        _ => {}
    }
}

/// Settings → AI changed the provider: store the same answer in the credential file, so the
/// gate does not ask again at the next start.
pub fn sync_choice(before: &crate::settings::Provider, after: &crate::settings::Provider) {
    if before != after {
        set_choice(Some(if *after == crate::settings::Provider::Publik { "publik" } else { "own_key" }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{LlmConfig, Provider, Settings};

    #[test]
    fn links_only_on_publikhq() {
        assert_eq!(publik_link(Some("https://publikhq.com/claim/AB12")), Some("https://publikhq.com/claim/AB12".into()));
        assert_eq!(publik_link(Some("https://evil.example/claim")), None);
        assert_eq!(publik_link(Some("http://publikhq.com/claim")), None);
        assert_eq!(publik_link(None), None);
    }

    #[test]
    fn usd_formatting() {
        assert_eq!(fmt_usd(0), "$0.00");
        assert_eq!(fmt_usd(50_000), "$0.05");
        assert_eq!(fmt_usd(400), "$0.0004");
        assert_eq!(fmt_usd(1_234_567), "$1.23");
    }

    #[test]
    fn balance_line_follows_the_plan_and_the_claim_state() {
        assert_eq!(balance_line(Some(410_000), "claimed", None), "$0.41 left");
        assert_eq!(balance_line(Some(0), "claimed", Some("Plus")), "Your plan: Plus");
        assert_eq!(balance_line(None, "anonymous", None), "$0.00 · sign in with publik to use it");
    }

    #[test]
    fn the_gate_screens_carry_the_contract_words() {
        let anonymous = json!({ "claim_state": "anonymous", "plan": { "kind": "none" }, "balance_micros": 0 });
        let s = gate_screen("sign_in", Some(&anonymous), None, None);
        assert_eq!(s.title, "Sign in with publik");
        assert_eq!(s.buttons, vec![("sign_in", "Sign in with publik")]);
        let s = gate_screen("unreachable", None, None, None);
        assert_eq!(s.body, "publik is not reachable. Check your connection.");
        assert_eq!(s.buttons, vec![("retry", "Retry")]);
        let empty = json!({ "claim_state": "claimed", "account": { "handle": "mann" }, "plan": { "kind": "none" }, "balance_micros": 0 });
        let s = gate_screen("choose", Some(&empty), None, None);
        assert_eq!(s.title, "Choose how to pay for AI");
        assert_eq!(s.body, "You are signed in as @mann. This app uses AI. Get a publik plan, or use your own key.");
        assert_eq!(s.buttons, vec![("get_plan", "Get a plan"), ("own_key", "Use my own key")]);
        let plan = json!({ "claim_state": "claimed", "account": { "handle": "mann" }, "plan": { "kind": "plan", "label": "Plus" }, "balance_micros": 0 });
        let s = gate_screen("choose", Some(&plan), None, None);
        assert_eq!(s.title, "Connect your plan");
        assert_eq!(s.body, "You are signed in as @mann. Your plan: Plus. Connect it to use AI in this app, or use your own key.");
        assert_eq!(s.buttons, vec![("connect", "Connect your plan"), ("own_key", "Use my own key")]);
    }

    #[test]
    fn a_402_renders_the_message_and_exactly_one_link() {
        let body = r#"{"error":{"type":"insufficient_credit","message":"Not enough publik balance. Link this computer and pick a plan at the link below.","available_micros":0,"claim_state":"anonymous","top_up_url":"https://publikhq.com/claim/HK7F-2QWD","claim_url":"https://publikhq.com/claim/HK7F-2QWD"}}"#;
        let e = map_error(402, &reqwest::header::HeaderMap::new(), body, None);
        assert_eq!(e.kind, "insufficient_credit");
        assert!(e.message.starts_with("Not enough publik balance"));
        assert_eq!(e.link_url.as_deref(), Some("https://publikhq.com/claim/HK7F-2QWD"));
        assert_eq!(e.link_label, CTA_SIGN_IN);
        // claimed → the add-plan label
        let body2 = r#"{"error":{"type":"insufficient_credit","message":"m","claim_state":"claimed","top_up_url":"https://publikhq.com/dashboard/api/add"}}"#;
        let e2 = map_error(402, &reqwest::header::HeaderMap::new(), body2, None);
        assert_eq!(e2.link_label, CTA_ADD_PLAN);
        // a link off publikhq.com is dropped, the dashboard stands in
        let body3 = r#"{"error":{"type":"insufficient_credit","message":"m","top_up_url":"https://phish.example/x"}}"#;
        let e3 = map_error(402, &reqwest::header::HeaderMap::new(), body3, None);
        assert_eq!(e3.link_url.as_deref(), Some(DASHBOARD_URL));
    }

    #[test]
    fn revoked_and_unavailable_map_to_their_kinds() {
        let e = map_error(401, &reqwest::header::HeaderMap::new(), r#"{"error":{"type":"key_revoked","reprovision":true}}"#, None);
        assert_eq!(e.kind, "key_revoked");
        assert!(e.reprovision);
        let e = map_error(401, &reqwest::header::HeaderMap::new(), "{}", None);
        assert_eq!(e.kind, "invalid_api_key");
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "30".parse().unwrap());
        let e = map_error(503, &h, "", None);
        assert_eq!(e.retry_after_secs, Some(30));
    }

    #[test]
    fn base_url_resolves_to_the_default() {
        assert_eq!(resolve_base_url("https://publikhq.com/api/v1/"), "https://publikhq.com/api/v1");
        assert_eq!(resolve_base_url(""), DEFAULT_BASE_URL);
    }

    #[test]
    fn the_choice_moves_the_provider_once() {
        let mut s = Settings::default();
        assert_eq!(s.llm.provider, Provider::Publik);
        apply_choice(&mut s, Some("own_key"));
        assert_eq!(s.llm.provider, Provider::OpenAI);
        assert_eq!(s.llm.api_key, "");
        apply_choice(&mut s, Some("publik"));
        assert_eq!(s.llm.provider, Provider::Publik);
        // An own key the person already pasted stays as it is.
        s.llm = LlmConfig { provider: Provider::Anthropic, base_url: "https://api.anthropic.com".into(), api_key: "sk-ant-x".into(), model: "m".into() };
        apply_choice(&mut s, Some("own_key"));
        assert_eq!(s.llm.provider, Provider::Anthropic);
        assert_eq!(s.llm.api_key, "sk-ant-x");
    }
}
