//! Pictures of the game's window with Windows Graphics Capture: started only when a board needs reading, frames
//! grabbed until two in a row look alike (the cards have landed), then stopped. The mouse is left out of the
//! picture, and on Windows 11 no yellow border is drawn.

use super::sys::Hwnd;
use vorax_scan::pixels::Img;
use windows::core::Interface;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

pub struct Capturer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    rt_device: IDirect3DDevice,
}

struct Session {
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

impl Capturer {
    pub fn new() -> windows::core::Result<Capturer> {
        unsafe {
            let mut device = None;
            let mut context = None;
            D3D11CreateDevice(None, D3D_DRIVER_TYPE_HARDWARE, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context))?;
            let device: ID3D11Device = device.unwrap();
            let dxgi: IDXGIDevice = device.cast()?;
            let rt_device: IDirect3DDevice = CreateDirect3D11DeviceFromDXGIDevice(&dxgi)?.cast()?;
            Ok(Capturer { device, context: context.unwrap(), rt_device })
        }
    }

    fn start(&self, hwnd: Hwnd) -> windows::core::Result<Session> {
        let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
        let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(hwnd.get())? };
        let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&self.rt_device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, item.Size()?)?;
        let session = pool.CreateCaptureSession(&item)?;
        let _ = session.SetIsCursorCaptureEnabled(false);
        let _ = session.SetIsBorderRequired(false);
        session.StartCapture()?;
        Ok(Session { pool, session })
    }

    /// The newest frame the session has, waiting up to `wait_ms` for one.
    fn latest(&self, s: &Session, wait_ms: u64) -> windows::core::Result<Option<Img>> {
        let t0 = std::time::Instant::now();
        loop {
            let mut newest = None;
            while let Ok(f) = s.pool.TryGetNextFrame() {
                newest = Some(f);
            }
            if let Some(frame) = newest {
                let img = self.to_img(&frame)?;
                let _ = frame.Close();
                return Ok(Some(img));
            }
            if t0.elapsed().as_millis() as u64 >= wait_ms {
                return Ok(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }

    fn to_img(&self, frame: &windows::Graphics::Capture::Direct3D11CaptureFrame) -> windows::core::Result<Img> {
        unsafe {
            let size = frame.ContentSize()?;
            let access: IDirect3DDxgiInterfaceAccess = frame.Surface()?.cast()?;
            let texture: ID3D11Texture2D = access.GetInterface()?;
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            desc.MiscFlags = 0;
            let mut staging = None;
            self.device.CreateTexture2D(&desc, None, Some(&mut staging))?;
            let staging = staging.unwrap();
            self.context.CopyResource(&staging, &texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            self.context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
            let (w, h) = ((size.Width as u32).min(desc.Width) as usize, (size.Height as u32).min(desc.Height) as usize);
            let mut img = Img::new(w, h);
            let src = std::slice::from_raw_parts(mapped.pData as *const u8, mapped.RowPitch as usize * h);
            for y in 0..h {
                let row = &src[y * mapped.RowPitch as usize..];
                for x in 0..w {
                    let (b, g, r) = (row[x * 4], row[x * 4 + 1], row[x * 4 + 2]);
                    img.data[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[r, g, b, 255]);
                }
            }
            self.context.Unmap(&staging, 0);
            Ok(img)
        }
    }

    /// One picture now (as the hotkey asks).
    pub fn grab(&self, hwnd: Hwnd) -> windows::core::Result<Option<Img>> {
        let s = self.start(hwnd)?;
        Ok(self.latest(&s, 1000)?.map(|img| client_only(hwnd, img)))
    }

    /// Pictures every 150 ms until two in a row look alike (the cards have stopped moving), at most 2.5 s.
    pub fn grab_stable(&self, hwnd: Hwnd) -> windows::core::Result<Option<Img>> {
        let s = self.start(hwnd)?;
        let t0 = std::time::Instant::now();
        let Some(mut frame) = self.latest(&s, 1000)? else { return Ok(None) };
        let mut prev = thumb(&frame);
        let mut still = 0;
        while t0.elapsed().as_millis() < 2500 {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let Some(f) = self.latest(&s, 500)? else { break };
            let t = thumb(&f);
            still = if frame_diff(&prev, &t) < 1.5 { still + 1 } else { 0 };
            frame = f;
            if still >= 2 {
                break;
            }
            prev = t;
        }
        Ok(Some(client_only(hwnd, frame)))
    }
}

/// The game's picture alone: a window capture also holds its title bar and borders (even ones off screen), so
/// the client area is cut out where Windows says it sits inside the captured frame.
fn client_only(hwnd: Hwnd, img: Img) -> Img {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    let Some((cx, cy, cw, ch)) = super::sys::client_rect(hwnd) else { return img };
    let mut frame = RECT::default();
    let ok = unsafe { DwmGetWindowAttribute(hwnd.get(), DWMWA_EXTENDED_FRAME_BOUNDS, &mut frame as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32) };
    if ok.is_err() {
        return img;
    }
    let (dx, dy) = (cx - frame.left, cy - frame.top);
    if dx < 0 || dy < 0 || (dx + cw) as usize > img.width || (dy + ch) as usize > img.height || (cw as usize == img.width && ch as usize == img.height) {
        return img;
    }
    vorax_scan::window::crop_to(&img, &vorax_scan::pixels::Rect::new(dx as f64, dy as f64, cw as f64, ch as f64))
}

/// A 96-pixel-wide grey thumbnail (src/solver/live/stable.ts): bubbles and flames barely move it, a dealt hand does.
fn thumb(img: &Img) -> (usize, Vec<f32>) {
    let w = 96usize;
    let h = ((img.height * w) as f64 / img.width as f64).round().max(1.0) as usize;
    let mut g = vec![0f32; w * h];
    let mut n = vec![0f32; w * h];
    let (sx, sy) = (w as f64 / img.width as f64, h as f64 / img.height as f64);
    // every 2nd pixel is plenty for a likeness test
    for y in (0..img.height).step_by(2) {
        let row = ((y as f64 * sy) as usize).min(h - 1) * w;
        for x in (0..img.width).step_by(2) {
            let i = (y * img.width + x) * 4;
            let t = row + ((x as f64 * sx) as usize).min(w - 1);
            g[t] += 0.299 * img.data[i] as f32 + 0.587 * img.data[i + 1] as f32 + 0.114 * img.data[i + 2] as f32;
            n[t] += 1.0;
        }
    }
    for i in 0..g.len() {
        g[i] /= n[i].max(1.0);
    }
    (h, g)
}

/// Mean absolute grey difference, 0-255.
fn frame_diff(a: &(usize, Vec<f32>), b: &(usize, Vec<f32>)) -> f32 {
    if a.1.len() != b.1.len() {
        return 255.0;
    }
    a.1.iter().zip(&b.1).map(|(x, y)| (x - y).abs()).sum::<f32>() / a.1.len() as f32
}
