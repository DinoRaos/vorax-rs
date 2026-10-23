//! The overlay window: transparent, click-through, always on top, never takes the focus. It covers the game's
//! picture and is redrawn only when the advice changes (or the game window moves), so it costs nothing in between.
//! Also the tray icon and the hotkeys.

use super::sys::{client_rect, find_game_window, foreground, note, Hwnd, Settings};
use super::menu::{put, Row};
use crate::draw::{render, Fonts, Plan};
use std::rc::Rc;
use std::cell::RefCell;
use std::sync::mpsc::{Receiver, Sender};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT};
use windows::Win32::UI::Shell::{Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW};
use windows::Win32::UI::WindowsAndMessaging::*;

/// What the worker tells the window.
pub enum ToUi {
    /// new marks, for a picture of this size
    Show(Plan, usize, usize),
    /// a board is being read: the last advice, dimmed, without its marks
    Busy,
    /// the last advice stands (the board didn't change)
    Unbusy,
    /// not the Surgical Preparation screen: nothing to show
    Clear,
    /// close the overlay (the demo's end)
    Quit,
    /// whether the game writes its moves to the log, which the overlay needs to read boards by itself
    GameLog(GameLog),
}

/// The game's log as the worker sees it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameLog {
    /// no UE_game.log where the game keeps it
    Missing,
    /// the game isn't open
    NoGame,
    /// the game just opened: no game line yet, but it's early to tell
    Checking,
    /// game lines are coming: boards are read by themselves
    On,
    /// the game is open but writes no game lines: its "Enable Log" is off
    Off,
}

/// What the window asks of the worker.
pub enum ToWorker {
    /// read the board now (Ctrl+Alt+X)
    Snap,
    /// the Intern Shelley level changed: advise the board again
    Shelley(i32),
    Quit,
}

const WM_WAKE: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const HK_TOGGLE: i32 = 1;
const HK_SNAP: i32 = 2;
const HK_QUIT: i32 = 3;
const TIMER_FOLLOW: usize = 1;
const MENU_TOGGLE: u32 = 10;
const MENU_SNAP: u32 = 11;
const MENU_LOG: u32 = 12;
const MENU_QUIT: u32 = 13;
/// Shelley levels 1-6 are MENU_SHELLEY + level
const MENU_SHELLEY: u32 = 20;
/// the marks' colours are MENU_ACCENT + their index in draw::ACCENTS
const MENU_ACCENT: u32 = 40;
const MENU_LATER: u32 = 16;
const MENU_LOG_HELP: u32 = 17;

struct State {
    hwnd: HWND,
    fonts: Rc<Fonts>,
    rx: Receiver<ToUi>,
    tx: Sender<ToWorker>,
    plan: Option<Plan>,
    busy: bool,
    /// the size of the picture the marks were laid out on
    frame: (usize, usize),
    settings: Settings,
    /// where the overlay last was, and whether it was showing
    placed: Option<(i32, i32, i32, i32)>,
    visible: bool,
    /// the game's log, once the worker has looked; whether its help card has shown by itself this run
    game_log: Option<GameLog>,
    log_help_shown: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// Wakes the window's thread from the worker, with its handle.
#[derive(Clone, Copy)]
pub struct Waker(Hwnd);
unsafe impl Send for Waker {}
impl Waker {
    pub fn wake(&self) {
        unsafe {
            let _ = PostMessageW(self.0.get(), WM_WAKE, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn create(fonts: Fonts, rx: Receiver<ToUi>, tx: Sender<ToWorker>, settings: Settings) -> windows::core::Result<Waker> {
    unsafe {
        let instance: HINSTANCE = GetModuleHandleW(None)?.into();
        let class = w!("VoraxRsOverlay");
        let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: instance, lpszClassName: class, hCursor: LoadCursorW(None, IDC_ARROW)?, ..Default::default() };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("Vorax overlay"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            instance,
            None,
        )?;
        for (id, vk) in [(HK_TOGGLE, 'V'), (HK_SNAP, 'X'), (HK_QUIT, 'Q')] {
            if RegisterHotKey(hwnd, id, HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_ALT.0 | MOD_NOREPEAT.0), vk as u32).is_err() {
                note(&format!("hotkey Ctrl+Alt+{vk} is taken by another program"));
            }
        }
        let mut nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP, uCallbackMessage: WM_TRAY, hIcon: app_icon(instance), ..Default::default() };
        let tip: Vec<u16> = "Vorax overlay (Ctrl+Alt+V show/hide)".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
        SetTimer(hwnd, TIMER_FOLLOW, 250, None);
        STATE.with(|s| {
            *s.borrow_mut() = Some(State { hwnd, fonts: Rc::new(fonts), rx, tx, plan: None, busy: false, frame: (0, 0), settings, placed: None, visible: false, game_log: None, log_help_shown: false });
        });
        Ok(Waker(Hwnd::of(hwnd)))
    }
}

/// The app's icon (assets/vorax.ico, in the .exe) at the tray's size for this screen; Windows' own without it.
unsafe fn app_icon(instance: HINSTANCE) -> HICON {
    let size = windows::Win32::UI::HiDpi::GetSystemMetricsForDpi(SM_CXSMICON, windows::Win32::UI::HiDpi::GetDpiForSystem());
    match LoadImageW(instance, PCWSTR(1 as *const u16), IMAGE_ICON, size, size, LR_DEFAULTCOLOR) {
        Ok(h) if !h.is_invalid() => HICON(h.0),
        _ => LoadIconW(None, IDI_APPLICATION).unwrap_or_default(),
    }
}

pub fn run_loop() {
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_WAKE => {
            with_state(|s| {
                let mut changed = false;
                while let Ok(m) = s.rx.try_recv() {
                    changed = true;
                    match m {
                        ToUi::Show(p, w, h) => {
                            s.plan = Some(p);
                            s.frame = (w, h);
                            s.busy = false;
                        }
                        ToUi::Busy => s.busy = true,
                        ToUi::Unbusy => s.busy = false,
                        ToUi::Clear => {
                            s.plan = None;
                            s.busy = false;
                        }
                        ToUi::Quit => unsafe {
                            let _ = DestroyWindow(s.hwnd);
                            return;
                        },
                        ToUi::GameLog(g) => {
                            s.game_log = Some(g);
                            // the steps by the tray, once a run, the first time the log turns out off
                            if g == GameLog::Off && !std::mem::replace(&mut s.log_help_shown, true) {
                                log_card(s, true);
                            }
                        }
                    }
                }
                if changed {
                    s.placed = None;
                    follow(s);
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_FOLLOW => {
            with_state(follow);
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wp.0 as i32 {
                HK_TOGGLE => {
                    with_state(toggle);
                }
                HK_SNAP => {
                    with_state(|s| s.tx.send(ToWorker::Snap).ok());
                }
                HK_QUIT => quit(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAY => {
            let event = (lp.0 & 0xffff) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                tray_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            command(hwnd, (wp.0 & 0xffff) as u32);
            LRESULT(0)
        }
        WM_DESTROY => {
            let nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn toggle(s: &mut State) {
    s.settings.shown = !s.settings.shown;
    s.settings.save();
    s.placed = None;
    follow(s);
}

fn set_shelley(s: &mut State, level: i32) {
    s.settings.shelley = level;
    s.settings.save();
    note(&format!("Intern Shelley level {level}"));
    let _ = s.tx.send(ToWorker::Shelley(level));
}

unsafe fn quit(hwnd: HWND) {
    with_state(|s| s.tx.send(ToWorker::Quit).ok());
    let _ = DestroyWindow(hwnd);
}

fn rerolls_hint(level: i32) -> String {
    let n = crate::pipeline::shelley_rerolls(level);
    format!("{n} instrument reroll{}", if n == 1 { "" } else { "s" })
}

/// Opens the tray menu (menu.rs), with the overlay's state as it is now.
fn tray_menu(hwnd: HWND) {
    let Some((rows, fonts)) = with_state(|s| {
        let mut rows = vec![Row::Header { title: "Vorax overlay".into(), status: format!("Odds of {}", vorax::data().odds) }];
        match s.game_log {
            Some(GameLog::On) => rows.push(Row::Text("Game log on: boards are read by themselves".into())),
            Some(GameLog::Off) => {
                rows.push(Row::Warn("Game log off: boards aren't read by themselves".into()));
                rows.push(Row::Action { id: MENU_LOG_HELP, label: "How to turn the game's log on".into(), shortcut: String::new() });
            }
            Some(GameLog::Missing) => {
                rows.push(Row::Warn("The game's log (UE_game.log) wasn't found".into()));
                rows.push(Row::Action { id: MENU_LOG_HELP, label: "What to do".into(), shortcut: String::new() });
            }
            Some(GameLog::Checking) => rows.push(Row::Text("Game log: checking…".into())),
            Some(GameLog::NoGame) | None => rows.push(Row::Text("Game log: waiting for the game".into())),
        }
        rows.push(Row::Separator);
        rows.push(Row::Switch { id: MENU_TOGGLE, label: "Show overlay".into(), on: s.settings.shown });
        rows.push(Row::Action { id: MENU_SNAP, label: "Read the board now".into(), shortcut: "Ctrl+Alt+X".into() });
        rows.push(Row::Levels { base: MENU_SHELLEY, level: s.settings.shelley, hint: rerolls_hint });
        rows.push(Row::Colors { base: MENU_ACCENT, selected: s.settings.accent });
        rows.push(Row::Separator);
        rows.push(Row::Action { id: MENU_LOG, label: "Open the debug log".into(), shortcut: String::new() });
        rows.push(Row::Separator);
        rows.push(Row::Action { id: MENU_QUIT, label: "Quit".into(), shortcut: "Ctrl+Alt+Q".into() });
        (rows, s.fonts.clone())
    }) else {
        return;
    };
    super::menu::open(hwnd, rows, fonts);
}

/// What to do when the game's log has no moves in it: the in-game switch, step by step. `by_tray`: shown by
/// itself, in the corner by the tray, without taking the game's focus.
fn log_card(s: &State, by_tray: bool) {
    let rows = if s.game_log == Some(GameLog::Missing) {
        vec![
            Row::Header { title: "Game log not found".into(), status: "Boards aren't read by themselves".into() },
            Row::Text("The overlay looks for UE_game.log in the".into()),
            Row::Text("game's Saved\\Logs folder. Start the game,".into()),
            Row::Text("or start the overlay with --log <path>.".into()),
            Row::Text("Meanwhile Ctrl+Alt+X reads the board.".into()),
            Row::Buttons { primary: (MENU_LATER, "OK".into()), secondary: None },
        ]
    } else {
        vec![
            Row::Header { title: "Turn the game's log on".into(), status: "Until then boards aren't read by themselves".into() },
            Row::Text("1.  In the game, press Esc".into()),
            Row::Text("2.  Open Account Related".into()),
            Row::Text("3.  Click Enable Log".into()),
            Row::Text("4.  Log out and back in".into()),
            Row::Text("Meanwhile Ctrl+Alt+X reads the board.".into()),
            Row::Buttons { primary: (MENU_LATER, "OK".into()), secondary: None },
        ]
    };
    if by_tray {
        super::menu::notify(s.hwnd, rows, s.fonts.clone());
    } else {
        super::menu::open(s.hwnd, rows, s.fonts.clone());
    }
}

/// A choice from the tray menu.
unsafe fn command(hwnd: HWND, id: u32) {
    match id {
        MENU_TOGGLE => {
            with_state(toggle);
        }
        MENU_SNAP => {
            with_state(|s| s.tx.send(ToWorker::Snap).ok());
        }
        MENU_LATER => {}
        MENU_LOG_HELP => {
            with_state(|s| log_card(s, false));
        }
        MENU_LOG => {
            let _ = std::process::Command::new("notepad.exe").arg(super::sys::data_dir().join("overlay.log")).spawn();
        }
        MENU_QUIT => quit(hwnd),
        c if (MENU_SHELLEY + 1..=MENU_SHELLEY + 6).contains(&c) => {
            with_state(|s| set_shelley(s, (c - MENU_SHELLEY) as i32));
        }
        c if (MENU_ACCENT..MENU_ACCENT + crate::draw::ACCENTS.len() as u32).contains(&c) => {
            with_state(|s| {
                s.settings.accent = (c - MENU_ACCENT) as usize;
                s.settings.save();
                s.placed = None;
                follow(s);
            });
        }
        _ => {}
    }
}

/// Keeps the overlay over the game's picture, shown only while the game is in front and there's advice.
fn follow(s: &mut State) {
    let game = find_game_window();
    let front = game.is_some_and(|g| foreground() == g);
    let rect = game.and_then(client_rect);
    let want = s.settings.shown && front && s.plan.is_some() && rect.is_some();
    unsafe {
        if !want {
            if s.visible {
                let _ = ShowWindow(s.hwnd, SW_HIDE);
                s.visible = false;
            }
            return;
        }
        let r = rect.unwrap();
        if s.placed != Some(r) {
            draw(s, r);
            s.placed = Some(r);
        }
        if !s.visible {
            let _ = ShowWindow(s.hwnd, SW_SHOWNOACTIVATE);
            s.visible = true;
        }
        let _ = SetWindowPos(s.hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Lays the marks out on the game's picture as it is now (scaled from the picture they were read on) and puts
/// them on screen.
fn draw(s: &State, (x, y, w, h): (i32, i32, i32, i32)) {
    let Some(plan) = &s.plan else { return };
    let (fw, fh) = s.frame;
    let (sx, sy) = if fw > 0 && fh > 0 { (w as f64 / fw as f64, h as f64 / fh as f64) } else { (1.0, 1.0) };
    let mut p = if s.busy { plan.dimmed() } else { plan.clone() };
    if let Some((c, cw, ch)) = &mut p.card {
        c.cx *= sx;
        c.cy *= sy;
        *cw *= sx;
        *ch *= sy;
    }
    if let Some(b) = &mut p.button {
        b.x *= sx;
        b.y *= sy;
        b.w *= sx;
        b.h *= sy;
    }
    for t in p.tanks.iter_mut() {
        t.x *= sx;
        t.y *= sy;
        t.w *= sx;
        t.h *= sy;
    }
    let pm = render(&p, w as u32, h as u32, &s.fonts, crate::draw::accent(s.settings.accent));
    unsafe { put(s.hwnd, x, y, w, h, pm.data()) };
}
