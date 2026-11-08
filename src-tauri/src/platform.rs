//! The operating-system layer under the host loop: which text field has focus,
//! its text, selection and on-screen geometry, and how to replace a range in it.
//!
//! macOS uses Accessibility (`ax.rs`); Windows uses UI Automation (`uia.rs`).
//! Every coordinate here is in logical points, origin at the top-left of the
//! primary display. Both sides give `Element` the same inherent methods
//! (`pid`, `same_as`, `identity_hash`) and this module the same free functions.

pub use crate::geom::Rect;

#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(windows)]
pub use crate::uia::{frontmost_app, is_trusted, mouse_location, post_mouse_move, prompt_trust, rects_for_range, replace_range, selection, text, visible_frame, Element, Watcher};
#[cfg(not(any(target_os = "macos", windows)))]
pub use other::*;

/// Our own app, as `frontmost_app` names it (bundle id on macOS, exe name on Windows).
#[cfg(target_os = "macos")]
pub const OUR_APP_ID: &str = "com.publik.grammarlama";
#[cfg(not(target_os = "macos"))]
pub const OUR_APP_ID: &str = "gspot.exe";

/// Whether the screen-reading permission is something the user grants (macOS
/// Accessibility). Windows UI Automation needs no grant.
pub const NEEDS_PERMISSION: bool = cfg!(target_os = "macos");

#[cfg(target_os = "macos")]
mod mac {
    use super::Rect;
    use crate::ax::{self, AXElement};

    pub type Element = AXElement;

    pub fn is_trusted() -> bool {
        ax::is_trusted()
    }

    pub fn prompt_trust() -> bool {
        ax::prompt_trust()
    }

    /// Frontmost application as `(pid, bundle id, localized name)`.
    pub fn frontmost_app() -> Option<(i32, String, String)> {
        ax::frontmost_app()
    }

    pub fn mouse_location() -> Option<(f64, f64)> {
        ax::mouse_location_top_left()
    }

    pub fn post_mouse_move(x: f64, y: f64) -> bool {
        ax::post_mouse_move(x, y)
    }

    /// Finds the focused text field of the app in front.
    pub struct Watcher {
        system: AXElement,
    }

    impl Watcher {
        pub fn new() -> Watcher {
            Watcher { system: AXElement::system_wide() }
        }

        /// The focused editable element of another process, with its role.
        pub fn focused_editable(&self) -> Option<(Element, String)> {
            let el = self.system.focused_element()?;
            let role = el.role().unwrap_or_default();
            if el.pid() != std::process::id() as i32 && is_editable(&el, &role) {
                Some((el, role))
            } else {
                None
            }
        }
    }

    pub fn text(el: &Element) -> Option<String> {
        el.string("AXValue")
    }

    /// `(location, length)` of the selection in UTF-16 units.
    pub fn selection(el: &Element) -> Option<(usize, usize)> {
        el.range("AXSelectedTextRange")
    }

    pub fn replace_range(el: &Element, pid: i32, loc: usize, len: usize, replacement: &str, before: &str) -> anyhow::Result<crate::apply::ApplyResult> {
        crate::apply::replace_range_ax(el, pid, loc, len, replacement, before)
    }

    fn is_editable(el: &AXElement, role: &str) -> bool {
        if role == "AXSecureTextField" {
            return false;
        }
        if matches!(role, "AXTextArea" | "AXTextField" | "AXComboBox" | "AXSearchField") {
            return true;
        }
        if role == "AXWebArea" || role == "AXWindow" || role == "AXApplication" {
            return false;
        }
        el.has_attr("AXSelectedTextRange") && el.is_settable("AXValue")
    }

    /// The element's frame clipped to its scroll area and window.
    pub fn visible_frame(el: &AXElement, frame: Rect) -> Rect {
        let mut vis = frame;
        if let Some(sa) = el.enclosing_scroll_area() {
            if let Some(f) = sa.frame() {
                if let Some(i) = vis.intersect(&f) {
                    vis = i;
                }
            }
        }
        if let Some(w) = el.window() {
            if let Some(f) = w.frame() {
                if let Some(i) = vis.intersect(&f) {
                    vis = i;
                }
            }
        }
        vis
    }

    /// Screen rects (one per visual line) for a UTF-16 range.
    pub fn rects_for_range(el: &AXElement, loc: usize, len: usize) -> Vec<Rect> {
        let mut out = Vec::new();
        if len == 0 {
            return out;
        }
        let first = el.line_for_index(loc);
        let last = el.line_for_index(loc + len - 1);
        if let (Some(l0), Some(l1)) = (first, last) {
            if l1 > l0 && l1 - l0 <= 40 {
                for line in l0..=l1 {
                    if let Some((ls, ll)) = el.range_for_line(line) {
                        let s = loc.max(ls);
                        let e = (loc + len).min(ls + ll);
                        if e > s {
                            if let Some(r) = el.bounds_for_range(s, e - s) {
                                if !r.is_empty() {
                                    out.push(r);
                                }
                            }
                        }
                    }
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
        if let Some(r) = el.bounds_for_range(loc, len) {
            if !r.is_empty() {
                out.push(r);
                return out;
            }
        }
        // Some text views answer an empty rect for a range that wraps lines (a
        // whole sentence): walk the range in short pieces and keep each piece's box.
        if len > 24 {
            let mut s = loc;
            while s < loc + len {
                let e = (s + 24).min(loc + len);
                if let Some(r) = el.bounds_for_range(s, e - s) {
                    if !r.is_empty() {
                        match out.last_mut() {
                            Some(last) if (last.y - r.y).abs() < 2.0 && r.x >= last.x => {
                                let right = (r.x + r.w).max(last.x + last.w);
                                last.w = right - last.x;
                                last.h = last.h.max(r.h);
                            }
                            _ => out.push(r),
                        }
                    }
                }
                s = e;
            }
            if out.is_empty() {
                log::debug!("no AX bounds for range {loc}+{len}");
            }
        }
        out
    }
}

/// Other systems (Linux): the app builds and its tests run, but there is no
/// text-field reader, so no document ever connects.
#[cfg(not(any(target_os = "macos", windows)))]
mod other {
    use super::Rect;

    #[derive(Clone, Debug)]
    pub struct Element;

    impl Element {
        pub fn pid(&self) -> i32 {
            0
        }
        pub fn same_as(&self, _: &Element) -> bool {
            false
        }
        pub fn identity_hash(&self) -> u64 {
            0
        }
        pub fn frame(&self) -> Option<Rect> {
            None
        }
    }

    pub struct Watcher;

    impl Watcher {
        pub fn new() -> Watcher {
            Watcher
        }
        pub fn focused_editable(&self) -> Option<(Element, String)> {
            None
        }
    }

    pub fn is_trusted() -> bool {
        false
    }
    pub fn prompt_trust() -> bool {
        false
    }
    pub fn frontmost_app() -> Option<(i32, String, String)> {
        None
    }
    pub fn mouse_location() -> Option<(f64, f64)> {
        None
    }
    pub fn post_mouse_move(_: f64, _: f64) -> bool {
        false
    }
    pub fn text(_: &Element) -> Option<String> {
        None
    }
    pub fn selection(_: &Element) -> Option<(usize, usize)> {
        None
    }
    pub fn visible_frame(_: &Element, frame: Rect) -> Rect {
        frame
    }
    pub fn rects_for_range(_: &Element, _: usize, _: usize) -> Vec<Rect> {
        vec![]
    }
    pub fn replace_range(_: &Element, _: i32, _: usize, _: usize, _: &str, _: &str) -> anyhow::Result<crate::apply::ApplyResult> {
        Err(anyhow::anyhow!("GSpot cannot edit text fields on this system."))
    }
}
