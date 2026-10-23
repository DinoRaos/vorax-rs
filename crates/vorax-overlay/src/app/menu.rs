//! The tray menu, drawn in the overlay's own style (dark, gold, rounded) instead of Windows' plain one. A small
//! layered window by the tray: hover lights a row, a click acts; the switch and the Shelley level change in place,
//! the other rows close it. Clicking elsewhere or Esc closes it too. Its choices go to the overlay window as
//! WM_COMMAND, with the ids window.rs answers.

use crate::draw::{measure, paint, rgba, rounded, text, Fonts, ACCENTS, GOLD, GOLD_DIM};
use std::cell::RefCell;
use std::rc::Rc;
use tiny_skia::{FillRule, PathBuilder, Pixmap, Stroke, Transform};
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone)]
pub enum Row {
    /// the title, with a status line under it (the odds in use)
    Header { title: String, status: String },
    /// a line of text
    Text(String),
    /// lines of text already wrapped to the card (`wrap`), an empty one a paragraph's gap
    Lines(Vec<String>),
    /// a progress bar, 0-1
    Progress(f32),
    /// one or two buttons at the right: the first filled (the main choice); commands close the card
    Buttons { primary: (u32, String), secondary: Option<(u32, String)> },
    /// a warning line (the game's log off or missing)
    Warn(String),
    Separator,
    /// a command; `shortcut` shown on the right
    Action { id: u32, label: String, shortcut: String },
    /// an on/off switch
    Switch { id: u32, label: String, on: bool },
    /// the Intern Shelley level, 1-6, as six buttons; `base` + level is the command
    Levels { base: u32, level: i32, hint: fn(i32) -> String },
    /// the marks' colour, as swatches; `base` + index is the command
    Colors { base: u32, selected: usize },
}

struct Menu {
    hwnd: HWND,
    owner: HWND,
    rows: Vec<Row>,
    fonts: Rc<Fonts>,
    s: f32,
    /// the row and, on the levels row, the level under the mouse
    hover: Option<(usize, Option<i32>)>,
}

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

/// WM_MOUSELEAVE (not in this version of the windows crate's WindowsAndMessaging)
const WM_MOUSELEAVE: u32 = 0x02A3;

const W: f32 = 312.0;
/// room around the card for its shadow
const MARGIN: f32 = 12.0;

fn height(r: &Row) -> f32 {
    match r {
        Row::Header { .. } => 58.0,
        Row::Warn(_) => 26.0,
        Row::Separator => 11.0,
        Row::Action { .. } | Row::Switch { .. } => 34.0,
        Row::Levels { .. } => 48.0,
        Row::Colors { .. } => 40.0,
        Row::Text(_) => 22.0,
        Row::Lines(l) => 8.0 + l.iter().map(|t| if t.is_empty() { 8.0 } else { 18.0 }).sum::<f32>(),
        Row::Progress(_) => 22.0,
        Row::Buttons { .. } => 52.0,
    }
}

/// Each row's top and bottom, in unscaled pixels from the card's top.
fn tops(rows: &[Row]) -> Vec<(f32, f32)> {
    let mut y = 6.0;
    rows.iter()
        .map(|r| {
            let t = (y, y + height(r));
            y += height(r);
            t
        })
        .collect()
}

fn card_h(rows: &[Row]) -> f32 {
    tops(rows).last().map_or(0.0, |t| t.1) + 6.0
}

/// The six level buttons' left edges (unscaled, from the card's left), and their size.
fn level_boxes() -> ([f32; 6], f32) {
    let (size, gap) = (24.0, 4.0);
    let right = W - 14.0;
    let left = right - 6.0 * size - 5.0 * gap;
    (std::array::from_fn(|i| left + i as f32 * (size + gap)), size)
}

/// The buttons' left edges and widths (unscaled, from the card's left): the main one at the right.
fn button_boxes(fonts: &Fonts, primary: &(u32, String), secondary: Option<&(u32, String)>) -> ((f32, f32), Option<(f32, f32)>) {
    let width = |t: &str| measure(&fonts.bold, t, 13.0) + 32.0;
    let pw = width(&primary.1);
    let p = (W - 14.0 - pw, pw);
    let s = secondary.map(|s| {
        let sw = width(&s.1);
        (p.0 - 8.0 - sw, sw)
    });
    (p, s)
}

/// The colour swatches' left edges (unscaled, from the card's left), and their size.
fn swatches() -> (Vec<f32>, f32) {
    let (size, gap) = (22.0, 8.0);
    let n = ACCENTS.len() as f32;
    let left = W - 14.0 - n * size - (n - 1.0) * gap;
    ((0..ACCENTS.len()).map(|i| left + i as f32 * (size + gap)).collect(), size)
}

fn render(m: &Menu) -> Pixmap {
    let s = m.s;
    let (w, h) = (W * s + 2.0 * MARGIN * s, card_h(&m.rows) * s + 2.0 * MARGIN * s);
    let mut pm = Pixmap::new(w.ceil() as u32, h.ceil() as u32).unwrap();
    let (ox, oy) = (MARGIN * s, MARGIN * s);
    let (cw, ch) = (W * s, card_h(&m.rows) * s);
    // a soft shadow, then the card
    for (grow, a) in [(10.0, 0.05), (6.0, 0.08), (3.0, 0.12)] {
        if let Some(p) = rounded(ox - grow * s, oy - grow * s + 3.0 * s, cw + 2.0 * grow * s, ch + 2.0 * grow * s, (12.0 + grow) * s) {
            pm.fill_path(&p, &paint(rgba(0, 0, 0, a)), FillRule::Winding, Transform::identity(), None);
        }
    }
    if let Some(p) = rounded(ox, oy, cw, ch, 12.0 * s) {
        pm.fill_path(&p, &paint(rgba(22, 17, 20, 0.985)), FillRule::Winding, Transform::identity(), None);
        let (r, g, b) = GOLD_DIM;
        pm.stroke_path(&p, &paint(rgba(r, g, b, 0.35)), &Stroke { width: 1.0 * s, ..Default::default() }, Transform::identity(), None);
    }
    let light = (241, 233, 226);
    let dim = (150, 138, 132);
    for (i, (row, (top, bottom))) in m.rows.iter().zip(tops(&m.rows)).enumerate() {
        let (y0, y1) = (oy + top * s, oy + bottom * s);
        let mid = (y0 + y1) / 2.0;
        let hovered = m.hover.is_some_and(|h| h.0 == i);
        let hover_bg = |pm: &mut Pixmap| {
            if let Some(p) = rounded(ox + 6.0 * s, y0 + 1.0 * s, cw - 12.0 * s, y1 - y0 - 2.0 * s, 7.0 * s) {
                let (r, g, b) = GOLD;
                pm.fill_path(&p, &paint(rgba(r, g, b, 0.10)), FillRule::Winding, Transform::identity(), None);
            }
        };
        match row {
            Row::Text(t) => text(&mut pm, &m.fonts.regular, t, 13.0 * s, ox + 16.0 * s, mid + 4.5 * s, (210, 200, 195), 1.0),
            Row::Lines(lines) => {
                let mut y = y0 + 4.0 * s;
                for t in lines {
                    if t.is_empty() {
                        y += 8.0 * s;
                        continue;
                    }
                    text(&mut pm, &m.fonts.regular, t, 12.5 * s, ox + 16.0 * s, y + 13.5 * s, (210, 200, 195), 1.0);
                    y += 18.0 * s;
                }
            }
            Row::Progress(f) => {
                let (bx, bw, bh) = (ox + 16.0 * s, cw - 32.0 * s, 6.0 * s);
                if let Some(p) = rounded(bx, mid - bh / 2.0, bw, bh, bh / 2.0) {
                    pm.fill_path(&p, &paint(rgba(255, 255, 255, 0.10)), FillRule::Winding, Transform::identity(), None);
                }
                let fw = (bw * f.clamp(0.0, 1.0)).max(bh);
                if let Some(p) = rounded(bx, mid - bh / 2.0, fw, bh, bh / 2.0) {
                    pm.fill_path(&p, &paint(rgba(GOLD.0, GOLD.1, GOLD.2, 0.95)), FillRule::Winding, Transform::identity(), None);
                }
            }
            Row::Buttons { primary, secondary } => {
                let (p, s2) = button_boxes(&m.fonts, primary, secondary.as_ref());
                let (bh, by) = (32.0 * s, mid - 16.0 * s);
                for (k, (bx, bw), label) in [(0, p, &primary.1)].into_iter().chain(s2.zip(secondary.as_ref()).map(|(b, l)| (1, b, &l.1))) {
                    let under = m.hover == Some((i, Some(k)));
                    if let Some(path) = rounded(ox + bx * s, by, bw * s, bh, 8.0 * s) {
                        let c = match (k, under) {
                            (0, false) => rgba(GOLD.0, GOLD.1, GOLD.2, 0.92),
                            (0, true) => rgba(GOLD.0, GOLD.1, GOLD.2, 1.0),
                            (_, false) => rgba(255, 255, 255, 0.08),
                            (_, true) => rgba(255, 255, 255, 0.16),
                        };
                        pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
                    }
                    let tw = measure(&m.fonts.bold, label, 13.0 * s);
                    text(&mut pm, &m.fonts.bold, label, 13.0 * s, ox + bx * s + (bw * s - tw) / 2.0, mid + 4.5 * s, if k == 0 { (30, 20, 14) } else { light }, 1.0);
                }
            }
            Row::Header { title, status } => {
                diamond(&mut pm, ox + 22.0 * s, y0 + 22.0 * s, 9.0 * s, 1.0);
                text(&mut pm, &m.fonts.bold, title, 15.0 * s, ox + 38.0 * s, y0 + 27.0 * s, light, 1.0);
                text(&mut pm, &m.fonts.regular, status, 12.0 * s, ox + 38.0 * s, y0 + 45.0 * s, dim, 1.0);
            }
            Row::Warn(t) => text(&mut pm, &m.fonts.regular, t, 12.0 * s, ox + 16.0 * s, mid + 4.0 * s, (240, 170, 90), 1.0),
            Row::Separator => {
                if let Some(p) = rounded(ox + 14.0 * s, mid, cw - 28.0 * s, 1.0 * s, 0.0) {
                    pm.fill_path(&p, &paint(rgba(255, 255, 255, 0.08)), FillRule::Winding, Transform::identity(), None);
                }
            }
            Row::Action { label, shortcut, .. } => {
                if hovered {
                    hover_bg(&mut pm);
                }
                text(&mut pm, &m.fonts.regular, label, 13.5 * s, ox + 16.0 * s, mid + 4.5 * s, light, 1.0);
                if !shortcut.is_empty() {
                    let tw = measure(&m.fonts.regular, shortcut, 12.0 * s);
                    text(&mut pm, &m.fonts.regular, shortcut, 12.0 * s, ox + cw - 16.0 * s - tw, mid + 4.0 * s, dim, 1.0);
                }
            }
            Row::Switch { label, on, .. } => {
                if hovered {
                    hover_bg(&mut pm);
                }
                text(&mut pm, &m.fonts.regular, label, 13.5 * s, ox + 16.0 * s, mid + 4.5 * s, light, 1.0);
                let (sw, sh) = (30.0 * s, 16.0 * s);
                let (sx, sy) = (ox + cw - 16.0 * s - sw, mid - sh / 2.0);
                if let Some(p) = rounded(sx, sy, sw, sh, sh / 2.0) {
                    let c = if *on { rgba(GOLD.0, GOLD.1, GOLD.2, 0.95) } else { rgba(255, 255, 255, 0.14) };
                    pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                }
                let knob = if *on { sx + sw - sh / 2.0 } else { sx + sh / 2.0 };
                if let Some(p) = PathBuilder::from_circle(knob, mid, sh / 2.0 - 2.5 * s) {
                    let c = if *on { rgba(30, 20, 14, 1.0) } else { rgba(220, 210, 204, 1.0) };
                    pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                }
            }
            Row::Colors { selected, .. } => {
                let under = m.hover.and_then(|h| if h.0 == i { h.1 } else { None });
                let name = ACCENTS[under.map_or(*selected, |u| u as usize).min(ACCENTS.len() - 1)].0;
                text(&mut pm, &m.fonts.regular, "Marker colour", 13.5 * s, ox + 16.0 * s, mid - 1.0 * s, light, 1.0);
                text(&mut pm, &m.fonts.regular, name, 11.5 * s, ox + 16.0 * s, mid + 13.0 * s, dim, 1.0);
                let (xs, size) = swatches();
                for (k, bx) in xs.iter().enumerate() {
                    let (cxp, r) = (ox + (bx + size / 2.0) * s, size * s / 2.0);
                    let (cr, cg, cb) = ACCENTS[k].1;
                    if k == *selected || under == Some(k as i32) {
                        if let Some(p) = PathBuilder::from_circle(cxp, mid, r + 3.0 * s) {
                            let a = if k == *selected { 0.95 } else { 0.4 };
                            pm.stroke_path(&p, &paint(rgba(255, 255, 255, a)), &Stroke { width: 1.6 * s, ..Default::default() }, Transform::identity(), None);
                        }
                    }
                    if let Some(p) = PathBuilder::from_circle(cxp, mid, r - 1.0 * s) {
                        pm.fill_path(&p, &paint(rgba(cr, cg, cb, 1.0)), FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
            Row::Levels { level, hint, .. } => {
                text(&mut pm, &m.fonts.regular, "Intern Shelley", 13.5 * s, ox + 16.0 * s, y0 + 21.0 * s, light, 1.0);
                let shown = m.hover.and_then(|h| if h.0 == i { h.1 } else { None }).unwrap_or(*level);
                text(&mut pm, &m.fonts.regular, &hint(shown), 11.5 * s, ox + 16.0 * s, y0 + 37.0 * s, dim, 1.0);
                let (xs, size) = level_boxes();
                for (k, bx) in xs.iter().enumerate() {
                    let l = k as i32 + 1;
                    let (x, y, sz) = (ox + bx * s, mid - size * s / 2.0, size * s);
                    let selected = l == *level;
                    let under = m.hover == Some((i, Some(l)));
                    if let Some(p) = rounded(x, y, sz, sz, 6.0 * s) {
                        let c = if selected {
                            rgba(GOLD.0, GOLD.1, GOLD.2, 0.95)
                        } else if under {
                            rgba(GOLD.0, GOLD.1, GOLD.2, 0.22)
                        } else {
                            rgba(255, 255, 255, 0.07)
                        };
                        pm.fill_path(&p, &paint(c), FillRule::Winding, Transform::identity(), None);
                    }
                    let n = l.to_string();
                    let font = if selected { &m.fonts.bold } else { &m.fonts.regular };
                    let tw = measure(font, &n, 12.5 * s);
                    text(&mut pm, font, &n, 12.5 * s, x + (sz - tw) / 2.0, mid + 4.5 * s, if selected { (30, 20, 14) } else { light }, 1.0);
                }
            }
        }
    }
    pm
}

fn diamond(pm: &mut Pixmap, x: f32, y: f32, r: f32, alpha: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x, y - r);
    pb.line_to(x + r * 0.8, y);
    pb.line_to(x, y + r);
    pb.line_to(x - r * 0.8, y);
    pb.close();
    if let Some(p) = pb.finish() {
        pm.fill_path(&p, &paint(rgba(GOLD.0, GOLD.1, GOLD.2, alpha)), FillRule::Winding, Transform::identity(), None);
    }
}

/// The row (and level) at a point of the window, in its pixels.
fn hit(m: &Menu, x: f32, y: f32) -> Option<(usize, Option<i32>)> {
    let s = m.s;
    let (cx, cy) = (x / s - MARGIN, y / s - MARGIN);
    if !(0.0..W).contains(&cx) {
        return None;
    }
    let i = tops(&m.rows).iter().position(|(t, b)| cy >= *t && cy < *b)?;
    match &m.rows[i] {
        Row::Action { .. } | Row::Switch { .. } => Some((i, None)),
        Row::Levels { .. } => {
            let (xs, size) = level_boxes();
            Some((i, xs.iter().position(|bx| cx >= *bx - 2.0 && cx < bx + size + 2.0).map(|k| k as i32 + 1)))
        }
        Row::Buttons { primary, secondary } => {
            let (p, s2) = button_boxes(&m.fonts, primary, secondary.as_ref());
            if cx >= p.0 && cx < p.0 + p.1 {
                Some((i, Some(0)))
            } else if s2.is_some_and(|b| cx >= b.0 && cx < b.0 + b.1) {
                Some((i, Some(1)))
            } else {
                Some((i, None))
            }
        }
        Row::Colors { .. } => {
            let (xs, size) = swatches();
            Some((i, xs.iter().position(|bx| cx >= *bx - 3.0 && cx < bx + size + 3.0).map(|k| k as i32)))
        }
        _ => None,
    }
}

fn redraw(m: &Menu) {
    let pm = render(m);
    let mut r = RECT::default();
    unsafe {
        let _ = GetWindowRect(m.hwnd, &mut r);
        put(m.hwnd, r.left, r.top, pm.width() as i32, pm.height() as i32, pm.data());
    }
}

/// `t` in lines that fit the card (12.5 px text), its line breaks kept; at most `max` lines, the last ending in "…".
pub fn wrap(fonts: &Fonts, t: &str, max: usize) -> Vec<String> {
    let room = W - 32.0;
    let mut out: Vec<String> = Vec::new();
    for para in t.lines() {
        if para.trim().is_empty() {
            if out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let mut line = String::new();
        for word in para.split_whitespace() {
            let next = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if measure(&fonts.regular, &next, 12.5) > room && !line.is_empty() {
                out.push(std::mem::replace(&mut line, word.to_string()));
            } else {
                line = next;
            }
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    if out.len() > max {
        out.truncate(max);
        if let Some(l) = out.last_mut() {
            l.push('…');
        }
    }
    out
}

/// New rows for the card that's open (a download's progress), kept at its bottom right corner: whether one was.
pub fn refresh(rows: Vec<Row>) -> bool {
    with(|m| {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(m.hwnd, &mut r);
        }
        m.rows = rows;
        m.hover = None;
        let pm = render(m);
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        unsafe { put(m.hwnd, r.right - w, r.bottom - h, w, h, pm.data()) };
        true
    })
    .unwrap_or(false)
}

/// Opens the menu by the cursor (above it, as the tray sits at the bottom), on top and focused.
pub fn open(owner: HWND, rows: Vec<Row>, fonts: Rc<Fonts>) {
    show(owner, rows, fonts, false);
}

/// Opens a card by itself, in the main screen's corner by the tray, on top but without taking the focus (the
/// game keeps it): a click on it or its button still answers, and clicking elsewhere after that closes it.
pub fn notify(owner: HWND, rows: Vec<Row>, fonts: Rc<Fonts>) {
    show(owner, rows, fonts, true);
}

fn show(owner: HWND, rows: Vec<Row>, fonts: Rc<Fonts>, by_tray: bool) {
    close();
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = w!("VoraxRsMenu");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: instance.into(), lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&wc);
        let mut cur = POINT::default();
        let _ = GetCursorPos(&mut cur);
        let mon = if by_tray { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) } else { MonitorFromPoint(cur, MONITOR_DEFAULTTONEAREST) };
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let s = dx as f32 / 96.0;
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let work = mi.rcWork;
        let mut m = Menu { hwnd: HWND::default(), owner, rows, fonts, s, hover: None };
        let pm = render(&m);
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        // the card's corner at the cursor (or the work area's), kept inside the screen's work area
        let margin = (MARGIN * s) as i32;
        let at = if by_tray { POINT { x: work.right, y: work.bottom } } else { cur };
        let x = (at.x - w + margin).clamp(work.left, work.right - w);
        let y = (at.y - h + margin).clamp(work.top, work.bottom - h);
        let Ok(hwnd) = CreateWindowExW(WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("Vorax menu"), WS_POPUP, x, y, w, h, None, None, instance, None) else { return };
        m.hwnd = hwnd;
        put(hwnd, x, y, w, h, pm.data());
        if by_tray {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        } else {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
        }
        MENU.with(|c| *c.borrow_mut() = Some(m));
    }
}

pub fn close() {
    let h = MENU.with(|c| c.borrow_mut().take().map(|m| m.hwnd));
    if let Some(h) = h {
        unsafe {
            let _ = DestroyWindow(h);
        }
    }
}

fn with<R>(f: impl FnOnce(&mut Menu) -> R) -> Option<R> {
    MENU.with(|c| c.borrow_mut().as_mut().map(f))
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let (x, y) = ((lp.0 & 0xffff) as i16 as f32, ((lp.0 >> 16) & 0xffff) as i16 as f32);
    match msg {
        WM_MOUSEMOVE => {
            with(|m| {
                let h = hit(m, x, y);
                if h != m.hover {
                    m.hover = h;
                    redraw(m);
                }
                let mut t = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut t);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|m| {
                if m.hover.take().is_some() {
                    redraw(m);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            // what was clicked, acted on once the menu's state is let go
            let act = with(|m| {
                let (i, level) = hit(m, x, y)?;
                match &mut m.rows[i] {
                    Row::Switch { id, on, .. } => {
                        *on = !*on;
                        let id = *id;
                        redraw(m);
                        Some((m.owner, id, false))
                    }
                    Row::Levels { base, level: cur, .. } => {
                        let l = level?;
                        *cur = l;
                        let id = *base + l as u32;
                        redraw(m);
                        Some((m.owner, id, false))
                    }
                    Row::Colors { base, selected } => {
                        let k = level? as usize;
                        *selected = k;
                        let id = *base + k as u32;
                        redraw(m);
                        Some((m.owner, id, false))
                    }
                    Row::Action { id, .. } => Some((m.owner, *id, true)),
                    Row::Buttons { primary, secondary } => match level? {
                        0 => Some((m.owner, primary.0, true)),
                        _ => secondary.as_ref().map(|s| (m.owner, s.0, true)),
                    },
                    _ => None,
                }
            })
            .flatten();
            if let Some((owner, id, closes)) = act {
                if closes {
                    close();
                }
                let _ = PostMessageW(owner, WM_COMMAND, WPARAM(id as usize), LPARAM(0));
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            close();
            LRESULT(0)
        }
        WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => {
            close();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// A premultiplied RGBA picture onto a layered window at a place on screen.
pub unsafe fn put(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, rgba: &[u8]) {
    let screen = GetDC(HWND::default());
    let mem = CreateCompatibleDC(screen);
    let bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
        ..Default::default()
    };
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    if let Ok(bmp) = CreateDIBSection(mem, &bi, DIB_RGB_COLORS, &mut bits, None, 0) {
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        for (d, s) in dst.chunks_exact_mut(4).zip(rgba.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let old = SelectObject(mem, bmp);
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let (pt, size, src) = (POINT { x, y }, SIZE { cx: w, cy: h }, POINT { x: 0, y: 0 });
        let _ = UpdateLayeredWindow(hwnd, screen, Some(&pt), Some(&size), mem, Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp);
    }
    let _ = DeleteDC(mem);
    ReleaseDC(HWND::default(), screen);
}

/// The menu drawn into a picture (for checking its look without opening it): `hover` lights that row.
pub fn preview(rows: Vec<Row>, fonts: Rc<Fonts>, s: f32, hover: Option<(usize, Option<i32>)>) -> Pixmap {
    render(&Menu { hwnd: HWND::default(), owner: HWND::default(), rows, fonts, s, hover })
}
