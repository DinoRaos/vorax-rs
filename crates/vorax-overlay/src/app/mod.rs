//! The live overlay on Windows: one window thread (the overlay, tray icon and hotkeys) and one worker thread
//! (the log, the pictures, the reading and the advice).

mod capture;
mod menu;
mod logview;
mod ocr;
mod sys;
mod update;
mod window;
mod worker;

use crate::arg;
use std::path::PathBuf;
use windows::core::w;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};

pub fn run(args: &[String]) {
    // Windows' shared COM area (the MTA) kept alive for the whole run, so a short-lived thread that used it can't
    // take it down under the factories the windows crate keeps cached
    unsafe {
        let _ = windows::Win32::System::Com::CoIncrementMTAUsage();
    }
    // a crash's message into the debug log (a release build has no console to show it)
    std::panic::set_hook(Box::new(|info| {
        let at = info.location().map(|l| format!(" at {}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        sys::note(&format!("CRASH: {msg}{at}"));
    }));
    // a release build has no console of its own: print into the one it was started from
    if args.iter().any(|a| a == "--selftest" || a == "--console" || a == "--demo") {
        unsafe {
            let _ = windows::Win32::System::Console::AttachConsole(windows::Win32::System::Console::ATTACH_PARENT_PROCESS);
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--demo") {
        return demo(args.get(i + 1).filter(|a| !a.starts_with("--")).map(PathBuf::from), arg(args, "--seconds").unwrap_or(15));
    }
    // the runs window drawn from the debug log into a picture
    if let Some(i) = args.iter().position(|a| a == "--runs-preview") {
        let Some(out) = args.get(i + 1) else { return };
        let Some(fonts) = crate::draw::Fonts::system() else { return };
        let log = std::fs::read_to_string(sys::data_dir().join("overlay.log")).unwrap_or_default();
        let pm = logview::preview(&log, std::rc::Rc::new(fonts), 1.0, 760.0);
        let mut rgba = pm.data().to_vec();
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as f32 / 255.0;
            for c in 0..3 {
                p[c] = (p[c] as f32 + 70.0 * (1.0 - a)).round() as u8;
            }
            p[3] = 255;
        }
        let _ = crate::pipeline::encode_png(std::path::Path::new(out), pm.width(), pm.height(), &rgba);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--menu-preview") {
        let Some(out) = args.get(i + 1) else { return };
        let Some(fonts) = crate::draw::Fonts::system() else { return };
        let rows = vec![
            menu::Row::Header { title: "Vorax overlay".into(), status: format!("v{} · odds of {}", update::current(), vorax::data().odds) },
            menu::Row::Warn("Game log off: boards aren't read by themselves".into()),
            menu::Row::Action { id: 0, label: "How to turn the game's log on".into(), shortcut: String::new() },
            menu::Row::Separator,
            menu::Row::Switch { id: 0, label: "Show overlay".into(), on: true },
            menu::Row::Action { id: 0, label: "Read the board now".into(), shortcut: "Ctrl+Alt+X".into() },
            menu::Row::Levels { base: 0, level: 6, hint: |l| format!("{} instrument rerolls", crate::pipeline::shelley_rerolls(l)) },
            menu::Row::Colors { base: 0, selected: 0 },
            menu::Row::Separator,
            menu::Row::Action { id: 0, label: "Your runs".into(), shortcut: "the log".into() },
            menu::Row::Separator,
            menu::Row::Action { id: 0, label: "Quit".into(), shortcut: "Ctrl+Alt+Q".into() },
        ];
        // the menu as it usually is: the game's log on
        let plain = args.iter().any(|a| a == "--plain");
        let rows = if plain {
            let mut head = vec![
                menu::Row::Header { title: "Vorax overlay".into(), status: format!("v{} · odds of {}", update::current(), vorax::data().odds) },
                menu::Row::Text("Game log on: boards are read by themselves".into()),
            ];
            head.extend(rows.into_iter().skip(3));
            head
        } else {
            rows
        };
        // the steps shown when the game's log is off
        let rows = if args.iter().any(|a| a == "--log-card") {
            vec![
                menu::Row::Header { title: "Turn the game's log on".into(), status: "Until then boards aren't read by themselves".into() },
                menu::Row::Text("1.  In the game, press Esc".into()),
                menu::Row::Text("2.  Open Account Related".into()),
                menu::Row::Text("3.  Click Enable Log".into()),
                menu::Row::Text("4.  Log out and back in".into()),
                menu::Row::Text("Meanwhile Ctrl+Alt+X reads the board.".into()),
                menu::Row::Buttons { primary: (0, "OK".into()), secondary: None },
            ]
        } else {
            rows
        };
        let hover = if plain { None } else { Some((6, Some(4))) };
        let pm = menu::preview(rows, std::rc::Rc::new(fonts), 1.5, hover);
        // onto a mid-grey, as a desktop would show it
        let mut rgba = pm.data().to_vec();
        for p in rgba.chunks_exact_mut(4) {
            let a = p[3] as f32 / 255.0;
            for c in 0..3 {
                p[c] = (p[c] as f32 + 70.0 * (1.0 - a)).round() as u8;
            }
            p[3] = 255;
        }
        let _ = crate::pipeline::encode_png(std::path::Path::new(out), pm.width(), pm.height(), &rgba);
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        return selftest(args.get(i + 1).map(PathBuf::from).unwrap_or_else(std::env::temp_dir));
    }
    if args.iter().any(|a| a == "--console") {
        std::env::set_var("VORAX_CONSOLE", "1");
    }
    if args.iter().any(|a| a == "--version") {
        return println!("vorax-overlay {}", update::current());
    }
    unsafe {
        // one overlay at a time (after an update, the one before is still closing: wait for it)
        let waits = if args.iter().any(|a| a == "--after-update") { 50 } else { 0 };
        let mut n = 0;
        loop {
            let h = CreateMutexW(None, true, w!("Local\\VoraxRsOverlay"));
            if GetLastError() != ERROR_ALREADY_EXISTS {
                break;
            }
            if let Ok(h) = h {
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
            n += 1;
            if n > waits {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        // physical pixels everywhere, so the overlay lines up with the captured picture at any display scaling
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let mut settings = sys::Settings::load();
    if let Some(s) = arg::<i32>(args, "--shelley") {
        settings.shelley = s.clamp(1, 6);
        settings.save();
    }
    if args.iter().any(|a| a == "--no-ocr") {
        settings.ocr = false;
    }
    // what earlier versions fetched from upstream and kept beside the settings: no longer used
    for old in ["data", "data.new", "sync-output.log"] {
        let p = sys::data_dir().join(old);
        let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
    }
    update::clean_up();
    sys::note(&format!("Vorax overlay v{} started (Intern Shelley level {}, odds of {})", update::current(), settings.shelley, vorax::data().odds));
    let Some(fonts) = crate::draw::Fonts::system() else {
        sys::note("no Segoe UI or Arial font found");
        return;
    };
    let (to_ui, from_worker) = std::sync::mpsc::channel();
    let (to_worker, from_ui) = std::sync::mpsc::channel();
    let waker = match window::create(fonts, from_worker, to_ui.clone(), to_worker, settings.clone(), args.to_vec()) {
        Ok(w) => w,
        Err(e) => {
            sys::note(&format!("overlay window: {e}"));
            return;
        }
    };
    let opts = worker::Options { log: arg::<String>(args, "--log").map(PathBuf::from), shelley: settings.shelley, ocr: settings.ocr, frames: arg::<String>(args, "--frames").map(PathBuf::from) };
    waker.wake();
    // a newer release on GitHub: offered in the tray menu
    let (check_tx, check_waker) = (to_ui.clone(), waker);
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
        }
        match update::check() {
            Ok(Some(r)) => {
                let _ = check_tx.send(window::ToUi::Update(r));
                check_waker.wake();
            }
            Ok(None) => sys::note(&format!("v{} is the latest release", update::current())),
            Err(e) => sys::note(&format!("couldn't check for a newer release: {e}")),
        }
    });
    let worker = std::thread::spawn(move || worker::run(opts, to_ui, waker, from_ui));
    window::run_loop();
    let _ = worker.join();
    sys::note("Vorax overlay closed");
}

/// Checks each piece on this PC without the overlay: the log, the game window, one picture of it, the text
/// reader on a known card title, and reading the picture. Writes the picture to `out`.
fn selftest(out: PathBuf) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED);
    }
    // the displays' colour mode: HDR changes the colours a capture gets
    unsafe {
        use windows::core::Interface;
        use windows::Win32::Graphics::Dxgi::*;
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut a = 0;
            while let Ok(adapter) = factory.EnumAdapters1(a) {
                let mut o = 0;
                while let Ok(output) = adapter.EnumOutputs(o) {
                    if let Ok(o6) = output.cast::<IDXGIOutput6>() {
                        if let Ok(d) = o6.GetDesc1() {
                            let name = String::from_utf16_lossy(&d.DeviceName).trim_end_matches('\0').to_string();
                            let hdr = d.ColorSpace == windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
                            println!("display {name}: {}x{}, HDR {}, colour space {}, max {} nits", d.DesktopCoordinates.right - d.DesktopCoordinates.left, d.DesktopCoordinates.bottom - d.DesktopCoordinates.top, if hdr { "ON" } else { "off" }, d.ColorSpace.0, d.MaxLuminance);
                        }
                    }
                    o += 1;
                }
                a += 1;
            }
        }
    }
    println!("log: {}", sys::find_log().map_or("not found".into(), |p| p.display().to_string()));
    let game = sys::find_game_window();
    println!("game window: {}", game.map_or("not open".into(), |g| format!("{:?}, picture at {:?}, in front: {}", g, sys::client_rect(g), sys::foreground() == g)));
    match ocr::Ocr::new() {
        Ok(o) => {
            let frame = crate::pipeline::decode_png(&PathBuf::from("upstream/tests/fixtures/frames/board-1080p.png"));
            match frame {
                Ok(f) => {
                    let l = vorax_scan::layout::layout(f.width, f.height);
                    for (j, c) in l.cards.iter().enumerate() {
                        let crop = vorax_scan::titles::title_crop(&f, c, l.k);
                        let t = std::time::Instant::now();
                        let text = o.read(&crop).unwrap_or_else(|e| format!("error {e}"));
                        println!("OCR ({}) card {j}: {:?} in {} ms", o.language, text.lines().next().unwrap_or(""), t.elapsed().as_millis());
                    }
                }
                Err(e) => println!("OCR: no test frame ({e})"),
            }
        }
        Err(e) => println!("OCR: not available ({e})"),
    }
    let Some(game) = game else { return };
    let cap = match capture::Capturer::new() {
        Ok(c) => c,
        Err(e) => return println!("capture: {e}"),
    };
    let t = std::time::Instant::now();
    match cap.grab(game) {
        Ok(Some(img)) => {
            println!("capture: {}x{} in {} ms", img.width, img.height, t.elapsed().as_millis());
            let path = out.join("selftest-capture.png");
            let _ = crate::pipeline::encode_png(&path, img.width as u32, img.height as u32, &img.data);
            println!("saved {}", path.display());
            let mut p = crate::pipeline::Pipeline::new(6);
            match p.run(&img, None, None) {
                crate::pipeline::Outcome::Advice { advice, plan, read_ms, .. } => println!("read in {read_ms:.0} ms, advice in {} ms: {:?}", advice.ms, plan.pill.map(|p| p.main)),
                crate::pipeline::Outcome::Animating => println!("read: a card is moving"),
                crate::pipeline::Outcome::NotGame => println!("read: not the Surgical Preparation screen (expected outside Vorax)"),
            }
        }
        Ok(None) => println!("capture: no frame"),
        Err(e) => println!("capture: {e}"),
    }
}

/// The overlay with a saved board's advice, over the game for 15 seconds (switch to the game to see it): checks
/// that it lines up on this screen without playing a Vorax round.
fn demo(frame: Option<PathBuf>, seconds: u64) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let path = frame.unwrap_or_else(|| PathBuf::from("upstream/tests/fixtures/frames/board-1080p.png"));
    let img = match crate::pipeline::decode_png(&path) {
        Ok(i) => i,
        Err(e) => return println!("{e}"),
    };
    let mut p = crate::pipeline::Pipeline::new(6);
    let crate::pipeline::Outcome::Advice { plan, .. } = p.run(&img, None, None) else { return println!("{}: no advice on that picture", path.display()) };
    println!("demo: {} over the game for {seconds} s", plan.pill.as_ref().map_or(String::new(), |p| format!("{} · {}", p.main, p.sub)));
    let Some(fonts) = crate::draw::Fonts::system() else { return };
    let (to_ui, from_worker) = std::sync::mpsc::channel();
    let (to_worker, _from_ui) = std::sync::mpsc::channel();
    let Ok(waker) = window::create(fonts, from_worker, to_ui.clone(), to_worker, sys::Settings { shown: true, ..sys::Settings::default() }, vec![]) else { return };
    let _ = to_ui.send(window::ToUi::Show(plan, img.width, img.height));
    waker.wake();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(seconds));
        let _ = to_ui.send(window::ToUi::Quit);
        waker.wake();
    });
    window::run_loop();
}
