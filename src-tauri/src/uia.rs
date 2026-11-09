//! Windows text-field access through UI Automation: the counterpart of `ax.rs`.
//! Also the Win32 pieces the host needs (the app in front, the pointer, the
//! display scale) and the Windows replacement ladder.
//!
//! Units: UI Automation and Win32 report physical pixels; everything leaving
//! this file is in logical points (pixels divided by the scale of the monitor
//! the point is on), which is what the overlay windows are placed in.
//!
//! Threads: every UI Automation call runs on a multithreaded-apartment thread
//! (the host loop, or a short-lived worker for a replacement), never on the UI
//! thread, so a slow or hung app cannot freeze GSpot's windows.

#![cfg(windows)]

use crate::apply::{ApplyResult, Method};
use crate::geom::Rect;
use crate::textmap::{provider_units, slice_utf16, splice_utf16};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;
use windows::core::{Interface, BSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{ClientToScreen, CombineRgn, CreateRectRgn, DeleteObject, MonitorFromPoint, SetWindowRgn, MONITOR_DEFAULTTONEAREST, RGN_OR};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, SAFEARRAY};
use windows::Win32::System::Ole::{SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound, SafeArrayUnaccessData};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::System::Variant::{VariantClear, VARIANT, VT_BOOL};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextRange, IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character, UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_IsReadOnlyAttributeId, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::Controls::{EM_REPLACESEL, EM_SETSEL};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_V};
use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetClassNameW, GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowRect, GetWindowThreadProcessId, SendMessageTimeoutW, SetCursorPos, SetForegroundWindow, GA_ROOT, SMTO_ABORTIFHUNG};

/// Text-unit conventions a provider may use for `TextUnit_Character`; found
/// per element by probing (see `detect_mode`).
const MODE_UNKNOWN: u8 = 0;
const MODE_UTF16: u8 = 1;
const MODE_CRLF_ONE: u8 = 2;
const MODE_CODEPOINT: u8 = 3;
const MODE_CRLF_ONE_CODEPOINT: u8 = 4;

fn mode_flags(mode: u8) -> (bool, bool) {
    match mode {
        MODE_CRLF_ONE => (true, false),
        MODE_CODEPOINT => (false, true),
        MODE_CRLF_ONE_CODEPOINT => (true, true),
        _ => (false, false),
    }
}

/// Start COM on this thread in the multithreaded apartment (no-op if it already is).
fn com_init() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn hwnd_of(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

/// Scale (1.0 = 96 DPI) of the monitor nearest a physical point.
fn scale_at(x: i32, y: i32) -> f64 {
    unsafe {
        let mon = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 {
            dx as f64 / 96.0
        } else {
            1.0
        }
    }
}

/// A physical-pixel rectangle in logical points (scale of the monitor under its top-left corner).
fn logical(x: f64, y: f64, w: f64, h: f64) -> Rect {
    let s = scale_at(x.round() as i32, y.round() as i32);
    Rect { x: x / s, y: y / s, w: w / s, h: h / s }
}

fn logical_rect(r: RECT) -> Rect {
    logical(r.left as f64, r.top as f64, (r.right - r.left) as f64, (r.bottom - r.top) as f64)
}

/// The values of a one-dimensional SAFEARRAY (destroyed afterwards).
unsafe fn take_safearray<T: Copy>(psa: *mut SAFEARRAY) -> Vec<T> {
    if psa.is_null() {
        return vec![];
    }
    let mut out = Vec::new();
    unsafe {
        let lb = SafeArrayGetLBound(psa, 1).unwrap_or(0);
        let ub = SafeArrayGetUBound(psa, 1).unwrap_or(-1);
        let n = (ub - lb + 1).max(0) as usize;
        let mut data: *mut c_void = std::ptr::null_mut();
        if n > 0 && SafeArrayAccessData(psa, &mut data).is_ok() && !data.is_null() {
            out = std::slice::from_raw_parts(data as *const T, n).to_vec();
            let _ = SafeArrayUnaccessData(psa);
        }
        let _ = SafeArrayDestroy(psa);
    }
    out
}

/// Whether this process may read other apps' text. UI Automation needs no grant on Windows.
pub fn is_trusted() -> bool {
    true
}

pub fn prompt_trust() -> bool {
    true
}

/// Window class name of a native window.
fn class_name(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// `(exe file name in lower case, display name)` for a process, cached by pid.
fn process_names(pid: u32) -> (String, String) {
    static CACHE: Mutex<Option<HashMap<u32, (String, String)>>> = Mutex::new(None);
    if let Some(v) = CACHE.lock().unwrap().as_ref().and_then(|m| m.get(&pid).cloned()) {
        return v;
    }
    let path = unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let mut buf = vec![0u16; 1024];
                let mut size = buf.len() as u32;
                let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size).is_ok();
                let _ = CloseHandle(h);
                if ok {
                    String::from_utf16_lossy(&buf[..size as usize])
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        }
    };
    let file = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
    let exe = file.to_lowercase();
    let name = file_description(&path).unwrap_or_else(|| {
        let stem = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(&file);
        let mut c = stem.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    });
    let v = (exe, name);
    let mut g = CACHE.lock().unwrap();
    let m = g.get_or_insert_with(HashMap::new);
    if m.len() > 128 {
        m.clear();
    }
    m.insert(pid, v.clone());
    v
}

/// The FileDescription in an exe's version resource ("Notepad", "Microsoft Edge").
fn file_description(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wide.as_ptr()), None, size, data.as_mut_ptr() as *mut c_void).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let tr: Vec<u16> = "\\VarFileInfo\\Translation\0".encode_utf16().collect();
        let mut keys = Vec::new();
        if VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(tr.as_ptr()), &mut ptr, &mut len).as_bool() && len >= 4 && !ptr.is_null() {
            let lang = *(ptr as *const u16);
            let cp = *(ptr as *const u16).add(1);
            keys.push(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        }
        keys.push("\\StringFileInfo\\040904b0\\FileDescription".into());
        keys.push("\\StringFileInfo\\040904e4\\FileDescription".into());
        for key in keys {
            let k: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
            let mut p: *mut c_void = std::ptr::null_mut();
            let mut n = 0u32;
            if VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(k.as_ptr()), &mut p, &mut n).as_bool() && n > 1 && !p.is_null() {
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, n as usize));
                let s = s.trim_end_matches('\0').trim().to_string();
                if !s.is_empty() && s.len() <= 80 {
                    return Some(s);
                }
            }
        }
    }
    None
}

/// Frontmost application as `(pid, exe file name, display name)`.
pub fn frontmost_app() -> Option<(i32, String, String)> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let (mut exe, name) = process_names(pid);
        // A classic console window is a terminal whatever program runs in it
        // (its window reports that program's process, not conhost.exe).
        if class_name(hwnd) == "ConsoleWindowClass" {
            exe = "conhost.exe".to_string();
        }
        Some((pid as i32, exe, name))
    }
}

/// The foreground window handle (0 when none).
pub fn foreground() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// Bring a window to the front. Windows allows it while this process owns the foreground.
pub fn set_foreground(h: isize) -> bool {
    if h == 0 {
        return false;
    }
    unsafe { SetForegroundWindow(hwnd_of(h)).as_bool() }
}

/// Give an overlay window the shape of `rects` (window-local physical pixels).
/// Outside the shape nothing is drawn and the pointer reaches the window below,
/// which is how the underline layer stays click-through on Windows: the usual
/// WS_EX_LAYERED route hides a WebView2 window. Skips the call when unchanged.
pub fn set_window_shape(h: isize, rects: &[(i32, i32, i32, i32)]) {
    static LAST: Mutex<Option<HashMap<isize, Vec<(i32, i32, i32, i32)>>>> = Mutex::new(None);
    if h == 0 {
        return;
    }
    {
        let mut g = LAST.lock().unwrap();
        let m = g.get_or_insert_with(HashMap::new);
        if m.get(&h).map(|v| v.as_slice() == rects).unwrap_or(false) {
            return;
        }
        m.insert(h, rects.to_vec());
    }
    unsafe {
        let rgn = CreateRectRgn(0, 0, 0, 0);
        for &(x, y, w, hh) in rects {
            if w <= 0 || hh <= 0 {
                continue;
            }
            let r = CreateRectRgn(x, y, x + w, y + hh);
            CombineRgn(Some(rgn), Some(rgn), Some(r), RGN_OR);
            let _ = DeleteObject(r.into());
        }
        // The window owns the region from here on.
        SetWindowRgn(hwnd_of(h), Some(rgn), true);
    }
}

/// Pointer position in logical points.
pub fn mouse_location() -> Option<(f64, f64)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    let s = scale_at(p.x, p.y);
    Some((p.x as f64 / s, p.y as f64 / s))
}

/// Move the pointer (logical points). Debug-probe only: lets the e2e harness hover an underline.
pub fn post_mouse_move(x: f64, y: f64) -> bool {
    let s = scale_at(x.round() as i32, y.round() as i32);
    unsafe { SetCursorPos((x * s).round() as i32, (y * s).round() as i32).is_ok() }
}

/// Finds the focused text field of the app in front. Lives on the host thread.
pub struct Watcher {
    auto: Option<IUIAutomation>,
    /// The element found last tick: reused while focus stays on it, so its
    /// patterns, unit probe and last text carry over between ticks.
    last: std::cell::RefCell<Option<(Element, String)>>,
}

impl Watcher {
    pub fn new() -> Watcher {
        com_init();
        let auto: Option<IUIAutomation> = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok() };
        if let Some(a2) = auto.as_ref().and_then(|a| a.cast::<IUIAutomation2>().ok()) {
            // A hung app must not stall the loop for UI Automation's 20 s default.
            unsafe {
                let _ = a2.SetConnectionTimeout(1500);
                let _ = a2.SetTransactionTimeout(1000);
            }
        }
        if auto.is_none() {
            log::error!("UI Automation is not available");
        }
        Watcher { auto, last: std::cell::RefCell::new(None) }
    }

    /// The focused editable text element of another process, with its role.
    pub fn focused_editable(&self) -> Option<(Element, String)> {
        let auto = self.auto.as_ref()?;
        unsafe {
            let el = auto.GetFocusedElement().ok()?;
            let pid = el.CurrentProcessId().ok()?;
            if pid <= 0 || pid as u32 == std::process::id() {
                return None;
            }
            if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) {
                return None;
            }
            let ct = el.CurrentControlType().ok()?;
            let tp: Option<IUIAutomationTextPattern> = el.GetCurrentPatternAs(UIA_TextPatternId).ok();
            let vp: Option<IUIAutomationValuePattern> = el.GetCurrentPatternAs(UIA_ValuePatternId).ok();
            if vp.as_ref().and_then(|v| v.CurrentIsReadOnly().ok()).map(|b| b.as_bool()).unwrap_or(false) {
                return None;
            }
            let role = if ct == UIA_EditControlTypeId {
                "Edit"
            } else if ct == UIA_DocumentControlTypeId {
                "Document"
            } else {
                "Text"
            };
            match (&tp, &vp) {
                (None, None) => return None,
                // A text-only element that is not an edit box: editable only when it says so.
                (Some(t), None) if role != "Edit" && text_read_only(t).unwrap_or(role != "Document") => return None,
                (None, Some(_)) if role != "Edit" => return None,
                _ => {}
            }
            let runtime_id: Vec<i32> = el.GetRuntimeId().map(|p| take_safearray::<i32>(p)).unwrap_or_default();
            if let Some((cached, role)) = self.last.borrow().as_ref() {
                if cached.0.pid == pid && !runtime_id.is_empty() && cached.0.runtime_id == runtime_id {
                    return Some((cached.clone(), role.clone()));
                }
            }
            let native = el.CurrentNativeWindowHandle().map(|h| h.0 as isize).unwrap_or(0);
            let top = {
                let fg = GetForegroundWindow();
                if !fg.0.is_null() {
                    fg.0 as isize
                } else if native != 0 {
                    GetAncestor(hwnd_of(native), GA_ROOT).0 as isize
                } else {
                    0
                }
            };
            let class = class_name(hwnd_of(native));
            log::info!("focused field: pid {pid}, class {class:?}, role {role}, text pattern {}, value pattern {}", tp.is_some(), vp.is_some());
            let inner = Inner { el, tp, vp, pid, runtime_id, native, top, class, mode: AtomicU8::new(MODE_UNKNOWN), last_text: Mutex::new(String::new()) };
            let found = (Element(Arc::new(inner)), role.to_string());
            *self.last.borrow_mut() = Some(found.clone());
            Some(found)
        }
    }
}

/// The document's IsReadOnly text attribute, when the provider answers it.
unsafe fn text_read_only(tp: &IUIAutomationTextPattern) -> Option<bool> {
    unsafe {
        let doc = tp.DocumentRange().ok()?;
        let mut v: VARIANT = doc.GetAttributeValue(UIA_IsReadOnlyAttributeId).ok()?;
        let out = if v.Anonymous.Anonymous.vt == VT_BOOL { Some(v.Anonymous.Anonymous.Anonymous.boolVal.0 != 0) } else { None };
        let _ = VariantClear(&mut v);
        out
    }
}

struct Inner {
    el: IUIAutomationElement,
    tp: Option<IUIAutomationTextPattern>,
    vp: Option<IUIAutomationValuePattern>,
    pid: i32,
    runtime_id: Vec<i32>,
    /// The element's own native window (0 for web content).
    native: isize,
    /// The top-level window it lives in (brought to the front before a paste).
    top: isize,
    class: String,
    /// How the provider counts characters (MODE_*), probed once per element.
    mode: AtomicU8,
    /// The last text read, for turning UTF-16 offsets into provider units.
    last_text: Mutex<String>,
}

/// A focused text element in another app.
#[derive(Clone)]
pub struct Element(Arc<Inner>);

// UI Automation client objects are free-threaded, and GSpot only touches them
// from multithreaded-apartment threads (see the module comment).
unsafe impl Send for Element {}
unsafe impl Sync for Element {}

impl std::fmt::Debug for Element {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Element(pid {}, {:?}, class {:?})", self.0.pid, self.0.runtime_id, self.0.class)
    }
}

impl Element {
    pub fn pid(&self) -> i32 {
        self.0.pid
    }

    pub fn same_as(&self, other: &Element) -> bool {
        self.0.pid == other.0.pid && !self.0.runtime_id.is_empty() && self.0.runtime_id == other.0.runtime_id
    }

    pub fn identity_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.0.runtime_id.hash(&mut h);
        h.finish()
    }

    /// A classic Win32 edit control (not rich edit): its own messages answer
    /// selection and layout when UI Automation has no text pattern for it.
    fn is_classic_edit(&self) -> bool {
        self.0.native != 0 && self.0.class.eq_ignore_ascii_case("edit")
    }

    pub fn frame(&self) -> Option<Rect> {
        let r = unsafe { self.0.el.CurrentBoundingRectangle().ok()? };
        let rect = logical_rect(r);
        (!rect.is_empty()).then_some(rect)
    }

    fn doc_range(&self) -> Option<IUIAutomationTextRange> {
        unsafe { self.0.tp.as_ref()?.DocumentRange().ok() }
    }

    /// A text range `start_u..start_u+len_u` in the provider's own character units.
    fn range_units(&self, start_u: usize, len_u: usize) -> Option<IUIAutomationTextRange> {
        let doc = self.doc_range()?;
        unsafe {
            let r = doc.Clone().ok()?;
            if start_u > 0 {
                let moved = r.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, start_u as i32).ok()?;
                if moved != start_u as i32 {
                    return None;
                }
            }
            r.MoveEndpointByRange(TextPatternRangeEndpoint_End, &r, TextPatternRangeEndpoint_Start).ok()?;
            if len_u > 0 {
                r.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, len_u as i32).ok()?;
            }
            Some(r)
        }
    }

    /// The range for a UTF-16 span of `text`, using the probed unit convention.
    fn range_utf16(&self, text: &str, loc: usize, len: usize) -> Option<IUIAutomationTextRange> {
        let (crlf, cp) = mode_flags(self.0.mode.load(Ordering::Relaxed));
        let s = provider_units(text, loc, crlf, cp);
        let e = provider_units(text, loc + len, crlf, cp);
        self.range_units(s, e.saturating_sub(s))
    }

    /// Find how this provider counts characters: probe the character right
    /// after the first CR LF or astral character under each convention.
    fn detect_mode(&self, text: &str) {
        if self.0.mode.load(Ordering::Relaxed) != MODE_UNKNOWN || self.0.tp.is_none() {
            return;
        }
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut probe = None;
        let mut i = 0usize;
        for ch in text.chars() {
            let l = ch.len_utf16();
            let after = i + l;
            let special = l == 2 || (ch == '\n' && i > 0 && units[i - 1] == '\r' as u16);
            if special && after < units.len() {
                probe = Some(after);
                break;
            }
            i = after;
        }
        let Some(at) = probe else { return };
        let want = slice_utf16(text, at, at + 1);
        for mode in [MODE_UTF16, MODE_CRLF_ONE, MODE_CODEPOINT, MODE_CRLF_ONE_CODEPOINT] {
            let (crlf, cp) = mode_flags(mode);
            let s = provider_units(text, at, crlf, cp);
            if let Some(r) = self.range_units(s, 1) {
                if unsafe { r.GetText(-1) }.map(|b| b.to_string()).ok().as_deref() == Some(want.as_str()) {
                    self.0.mode.store(mode, Ordering::Relaxed);
                    log::info!("text units for {}: mode {mode}", self.0.class);
                    return;
                }
            }
        }
        self.0.mode.store(MODE_UTF16, Ordering::Relaxed);
    }
}

/// The whole text of the field.
pub fn text(el: &Element) -> Option<String> {
    let t = match el.doc_range() {
        Some(doc) => unsafe { doc.GetText(-1).ok()?.to_string() },
        None => unsafe { el.0.vp.as_ref()?.CurrentValue().ok()?.to_string() },
    };
    el.detect_mode(&t);
    *el.0.last_text.lock().unwrap() = t.clone();
    Some(t)
}

/// `(location, length)` of the selection in UTF-16 units of `text(el)`.
pub fn selection(el: &Element) -> Option<(usize, usize)> {
    selection_uia(el).or_else(|| if el.is_classic_edit() { edit_selection(el.0.native) } else { None })
}

fn selection_uia(el: &Element) -> Option<(usize, usize)> {
    let tp = el.0.tp.as_ref()?;
    unsafe {
        let sel = tp.GetSelection().ok()?;
        if sel.Length().ok()? < 1 {
            return None;
        }
        let r = sel.GetElement(0).ok()?;
        let before = tp.DocumentRange().ok()?;
        before.MoveEndpointByRange(TextPatternRangeEndpoint_End, &r, TextPatternRangeEndpoint_Start).ok()?;
        let start = before.GetText(-1).ok()?.to_string().encode_utf16().count();
        let len = r.GetText(-1).ok()?.to_string().encode_utf16().count();
        Some((start, len))
    }
}

/// The element's frame clipped to its top-level window.
pub fn visible_frame(el: &Element, frame: Rect) -> Rect {
    let mut vis = frame;
    if el.0.top != 0 {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(hwnd_of(el.0.top), &mut r) }.is_ok() {
            if let Some(i) = vis.intersect(&logical_rect(r)) {
                vis = i;
            }
        }
    }
    vis
}

/// Screen rects (one per visual line) for a UTF-16 range of the last text read.
pub fn rects_for_range(el: &Element, loc: usize, len: usize) -> Vec<Rect> {
    if len == 0 {
        return vec![];
    }
    let text = el.0.last_text.lock().unwrap().clone();
    let from_uia: Vec<Rect> = match el.range_utf16(&text, loc, len) {
        Some(r) => {
            let nums: Vec<f64> = unsafe { r.GetBoundingRectangles().map(|p| take_safearray::<f64>(p)).unwrap_or_default() };
            nums.chunks_exact(4).map(|c| logical(c[0], c[1], c[2], c[3])).filter(|r| !r.is_empty()).collect()
        }
        None => vec![],
    };
    if from_uia.is_empty() && el.is_classic_edit() {
        return edit_rects(el.0.native, text.encode_utf16().count(), loc, len);
    }
    from_uia
}

// Classic Win32 edit control messages (winuser.h). The system marshals these
// across processes; each answer is a plain value, no pointers.
const EM_GETSEL: u32 = 0x00B0;
const EM_GETLINECOUNT: u32 = 0x00BA;
const EM_LINEINDEX: u32 = 0x00BB;
const EM_LINELENGTH: u32 = 0x00C1;
const EM_LINEFROMCHAR: u32 = 0x00C9;
const EM_POSFROMCHAR: u32 = 0x00D6;

/// The selection of a classic edit control (its own character indexes, which
/// are the UTF-16 units of its text, CR LF counted as two).
fn edit_selection(h: isize) -> Option<(usize, usize)> {
    let r = send_message(h, EM_GETSEL, 0, 0)? as isize;
    if r == -1 {
        return None; // 64K or more: the packed answer cannot say
    }
    let (start, end) = ((r & 0xFFFF) as usize, ((r >> 16) & 0xFFFF) as usize);
    (end >= start).then_some((start, end - start))
}

/// Client position of character `i` of a classic edit control (None past the end).
fn edit_pos(h: isize, i: usize) -> Option<(i32, i32)> {
    let r = send_message(h, EM_POSFROMCHAR, i, 0)? as isize;
    if r == -1 {
        return None;
    }
    Some(((r & 0xFFFF) as u16 as i16 as i32, ((r >> 16) & 0xFFFF) as u16 as i16 as i32))
}

/// Screen rects (one per line) for characters `loc..loc+len` of a classic edit
/// control, from the control's own layout: for fields whose UI Automation
/// provider has no text pattern (classic Notepad on Windows Server 2022/2025).
fn edit_rects(h: isize, text_len: usize, loc: usize, len: usize) -> Vec<Rect> {
    if h == 0 || len == 0 || loc + len > text_len {
        return vec![];
    }
    let line_of = |i: usize| send_message(h, EM_LINEFROMCHAR, i, 0);
    let lines = send_message(h, EM_GETLINECOUNT, 0, 0).unwrap_or(1).max(1);
    let line_top = |l: usize| -> Option<i32> { edit_pos(h, send_message(h, EM_LINEINDEX, l, 0)?).map(|p| p.1) };
    // Line height: the distance between two neighbouring lines, else the client height (one-line field).
    let mut client = RECT::default();
    let _ = unsafe { GetClientRect(hwnd_of(h), &mut client) };
    let lh = if lines > 1 {
        match (line_top(0), line_top(1)) {
            (Some(a), Some(b)) if b > a => b - a,
            _ => 16,
        }
    } else {
        (client.bottom - client.top).clamp(8, 40)
    };
    let (Some(first), Some(last)) = (line_of(loc), line_of(loc + len - 1)) else { return vec![] };
    let mut out = Vec::new();
    for l in first..=last.min(first + 50) {
        let Some(ls) = send_message(h, EM_LINEINDEX, l, 0) else { continue };
        let ll = send_message(h, EM_LINELENGTH, ls, 0).unwrap_or(0);
        let (s, e) = (loc.max(ls), (loc + len).min(ls + ll));
        if e <= s {
            continue;
        }
        let Some((x0, y0)) = edit_pos(h, s) else { continue };
        // Right edge: where the next character starts on this line; else the last
        // character's start plus the average width of the span.
        let x1 = match edit_pos(h, e) {
            Some((x, y)) if y == y0 && x > x0 => x,
            _ => {
                let lx = edit_pos(h, e - 1).map(|p| p.0).unwrap_or(x0);
                let avg = if e - s > 1 { ((lx - x0) as f64 / (e - s - 1) as f64).round() as i32 } else { 8 };
                lx + avg.max(4)
            }
        };
        let mut p = POINT { x: x0, y: y0 };
        if !unsafe { ClientToScreen(hwnd_of(h), &mut p) }.as_bool() {
            continue;
        }
        let r = logical(p.x as f64, p.y as f64, (x1 - x0) as f64, lh as f64);
        if !r.is_empty() {
            out.push(r);
        }
    }
    out
}

fn verified(el: &Element, loc: usize, replacement: &str) -> Option<String> {
    let now = text(el)?;
    let got = slice_utf16(&now, loc, loc + replacement.encode_utf16().count());
    (got == replacement).then_some(now)
}

fn send_message(hwnd: isize, msg: u32, w: usize, l: isize) -> Option<usize> {
    let mut result = 0usize;
    let r = unsafe { SendMessageTimeoutW(hwnd_of(hwnd), msg, WPARAM(w), LPARAM(l), SMTO_ABORTIFHUNG, 1500, Some(&mut result)) };
    (r.0 != 0).then_some(result)
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
    }
}

fn paste_text(text: &str) -> Result<()> {
    use arboard::Clipboard;
    let mut cb = Clipboard::new()?;
    let saved = cb.get_text().ok();
    cb.set_text(text.to_string())?;
    sleep(Duration::from_millis(60));
    let inputs = [key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)];
    unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sleep(Duration::from_millis(200));
    if let Some(prev) = saved {
        let _ = cb.set_text(prev);
    }
    Ok(())
}

/// Replace `len` UTF-16 units at `loc` with `replacement` in the host field.
/// Runs on its own multithreaded-apartment thread (the caller may be the UI thread).
pub fn replace_range(el: &Element, _pid: i32, loc: usize, len: usize, replacement: &str, before: &str) -> Result<ApplyResult> {
    let (el, replacement, before) = (el.clone(), replacement.to_string(), before.to_string());
    std::thread::Builder::new()
        .name("replace".into())
        .spawn(move || {
            com_init();
            ladder(&el, loc, len, &replacement, &before)
        })?
        .join()
        .map_err(|_| anyhow!("The replacement stopped unexpectedly."))?
}

fn ladder(el: &Element, loc: usize, len: usize, replacement: &str, before: &str) -> Result<ApplyResult> {
    // Guard: the field must still contain what we think it does.
    let live = text(el).ok_or_else(|| anyhow!("The text field is no longer available."))?;
    if live != before {
        let want = slice_utf16(before, loc, loc + len);
        if slice_utf16(&live, loc, loc + len) != want {
            return Err(anyhow!("The text changed since this suggestion was made. Try again."));
        }
    }
    let expected = splice_utf16(&live, loc, len, replacement);

    // Rung 1: a Win32 edit or rich-edit control. Select and replace with the
    // control's own messages: no focus change, no clipboard, and Ctrl+Z undoes it.
    let class = el.0.class.to_ascii_lowercase();
    if el.0.native != 0 && (class == "edit" || class.starts_with("richedit")) && send_message(el.0.native, EM_SETSEL, loc, (loc + len) as isize).is_some() {
        if selection(el) == Some((loc, len)) {
            let wide: Vec<u16> = replacement.encode_utf16().chain(std::iter::once(0)).collect();
            if send_message(el.0.native, EM_REPLACESEL, 1, wide.as_ptr() as isize).is_some() {
                sleep(Duration::from_millis(60));
                if let Some(new_text) = verified(el, loc, replacement) {
                    return Ok(ApplyResult { method: Method::Ax, new_text });
                }
            }
        } else {
            log::info!("edit control selection did not land on {loc}+{len}; trying UI Automation");
        }
    }

    // Rung 2: select the range through UI Automation, bring the window to the
    // front and paste (what Grammarly calls "hybrid replacements").
    if let Some(r) = el.range_utf16(&live, loc, len) {
        if unsafe { r.Select() }.is_ok() {
            sleep(Duration::from_millis(50));
            if selection(el) == Some((loc, len)) {
                if foreground() != el.0.top {
                    set_foreground(el.0.top);
                    sleep(Duration::from_millis(150));
                }
                if paste_text(replacement).is_ok() {
                    for _ in 0..6 {
                        sleep(Duration::from_millis(80));
                        if let Some(new_text) = verified(el, loc, replacement) {
                            return Ok(ApplyResult { method: Method::Paste, new_text });
                        }
                    }
                    if let Some(now) = text(el) {
                        if now == expected {
                            return Ok(ApplyResult { method: Method::Paste, new_text: now });
                        }
                    }
                }
            }
        }
    }

    // Rung 3: a plain field with a writable value: set the whole value.
    if let Some(vp) = el.0.vp.as_ref() {
        let read_only = unsafe { vp.CurrentIsReadOnly() }.map(|b| b.as_bool()).unwrap_or(true);
        if !read_only && expected.len() <= 20_000 && unsafe { vp.SetValue(&BSTR::from(expected.as_str())) }.is_ok() {
            sleep(Duration::from_millis(60));
            if let Some(new_text) = verified(el, loc, replacement) {
                return Ok(ApplyResult { method: Method::Ax, new_text });
            }
        }
    }
    Err(anyhow!("This app did not accept the replacement. Try editing it directly."))
}
