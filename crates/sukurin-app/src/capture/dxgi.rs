//! Desktop Duplication capture. A duplication session per output stays open
//! for the life of the app; a grab copies the newest desktop image that the
//! GPU already holds, which keeps hotkey-to-pixels around 10 ms where a GDI
//! BitBlt of the same display costs ~50 ms.

use super::hdr::ToneMap;
use anyhow::{Context, Result, anyhow};
use shattr_core::{Bitmap, Display, RectPx};
use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SDR_WHITE_LEVEL, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DisplayConfigGetDeviceInfo,
    GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::core::Interface;

pub struct Output {
    pub display: Display,
    adapter: IDXGIAdapter1,
    output: IDXGIOutput1,
    session: Option<Session>,
}

/// How the desktop surface is encoded, which decides the capture format.
enum Surface {
    /// An 8-bit sRGB desktop: copied as is.
    Sdr,
    /// An HDR desktop composed in scRGB. Captured as FP16 and mapped back to
    /// sRGB at the user's SDR content brightness, because 8-bit duplication
    /// of an HDR desktop washes SDR colours out (#0F0F0F comes back #262626
    /// at a 320-nit SDR white).
    Hdr(ToneMap),
}

impl Surface {
    fn format(&self) -> Common::DXGI_FORMAT {
        match self {
            Surface::Sdr => Common::DXGI_FORMAT_B8G8R8A8_UNORM,
            Surface::Hdr(_) => Common::DXGI_FORMAT_R16G16B16A16_FLOAT,
        }
    }

    fn bytes_per_pixel(&self) -> usize {
        match self {
            Surface::Sdr => 4,
            Surface::Hdr(_) => 8,
        }
    }
}

struct Session {
    device: ID3D11Device,
    ctx: ID3D11DeviceContext,
    dup: IDXGIOutputDuplication,
    latest: ID3D11Texture2D,
    /// CPU-readable copy of `latest`, created by the first grab and dropped
    /// by `trim`. A full-display FP16 one is 31 MiB of resident memory.
    staging: Option<ID3D11Texture2D>,
    width: u32,
    height: u32,
    surface: Surface,
    device_name: String,
    /// False until a frame carrying a desktop image has been copied into
    /// `latest`. The first frames of a session can be metadata-only.
    has_image: bool,
}

pub fn outputs() -> Result<Vec<Output>> {
    let mut found = Vec::new();
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
        let mut ai = 0;
        while let Ok(adapter) = factory.EnumAdapters1(ai) {
            ai += 1;
            let mut oi = 0;
            while let Ok(output) = adapter.EnumOutputs(oi) {
                oi += 1;
                let desc = output.GetDesc()?;
                if !desc.AttachedToDesktop.as_bool() {
                    continue;
                }
                let r = desc.DesktopCoordinates;
                found.push(Output {
                    display: Display {
                        rect: RectPx::new(r.left, r.top, (r.right - r.left) as u32, (r.bottom - r.top) as u32),
                        scale: monitor_scale(desc.Monitor),
                        name: wide(&desc.DeviceName),
                    },
                    adapter: adapter.clone(),
                    output: output.cast()?,
                    session: None,
                });
            }
        }
    }
    if found.is_empty() {
        return Err(anyhow!("no DXGI outputs attached to the desktop"));
    }
    Ok(found)
}

fn wide(s: &[u16]) -> String {
    String::from_utf16_lossy(s).trim_end_matches('\0').to_string()
}

fn monitor_scale(monitor: HMONITOR) -> f32 {
    let (mut x, mut y) = (96u32, 96u32);
    unsafe {
        if GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y).is_err() {
            return 1.0;
        }
    }
    x as f32 / 96.0
}

fn is_hdr(output: &IDXGIOutput1) -> bool {
    output
        .cast::<IDXGIOutput6>()
        .and_then(|o| unsafe { o.GetDesc1() })
        .is_ok_and(|d| d.ColorSpace == Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020)
}

/// The "SDR content brightness" of the display named `gdi_name`, in scRGB
/// units (nits / 80). 1.0 if Windows won't say.
fn sdr_white(gdi_name: &str) -> f32 {
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
            return 1.0;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
            return 1.0;
        }
        for p in &paths[..np as usize] {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            source.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            source.header.adapterId = p.sourceInfo.adapterId;
            source.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut source.header) != 0 || wide(&source.viewGdiDeviceName) != gdi_name {
                continue;
            }
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL::default();
            white.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL;
            white.header.size = std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32;
            white.header.adapterId = p.targetInfo.adapterId;
            white.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut white.header) == 0 {
                return white.SDRWhiteLevel as f32 / 1000.0;
            }
        }
        1.0
    }
}

impl Output {
    /// Opens the duplication session ahead of the first hotkey so the first
    /// grab doesn't pay the ~150 ms setup.
    pub fn warm(&mut self) -> Result<()> {
        if self.session.is_none() {
            self.session = Some(Session::open(&self.adapter, &self.output, &self.display.name)?);
        }
        Ok(())
    }

    pub fn grab(&mut self) -> Result<Bitmap> {
        for attempt in 0..2 {
            self.warm()?;
            match self.session.as_mut().unwrap().grab() {
                Ok(b) => return Ok(b),
                Err(e) if attempt == 0 => {
                    log::warn!("dxgi grab failed, reopening session: {e:#}");
                    self.session = None;
                }
                Err(e) => return Err(e),
            }
        }
        unreachable!()
    }

    pub fn trim(&mut self) {
        if let Some(s) = &mut self.session {
            s.trim();
        }
    }
}

impl Session {
    fn open(adapter: &IDXGIAdapter1, output: &IDXGIOutput1, device_name: &str) -> Result<Session> {
        let surface = if is_hdr(output) {
            let white = sdr_white(device_name);
            log::info!("{device_name} is HDR; SDR white {:.0} nits", white * 80.0);
            Surface::Hdr(ToneMap::new(white))
        } else {
            Surface::Sdr
        };
        unsafe {
            let (mut device, mut ctx) = (None, None);
            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut ctx),
            )
            .context("D3D11CreateDevice")?;
            let device = device.unwrap();
            let ctx = ctx.unwrap();
            let dup = match output.cast::<IDXGIOutput5>() {
                Ok(o5) => o5.DuplicateOutput1(&device, 0, &[surface.format()]),
                Err(_) => output.DuplicateOutput(&device),
            }
            .context("DuplicateOutput")?;
            let mode = dup.GetDesc().ModeDesc;
            let (width, height) = (mode.Width, mode.Height);

            let desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: surface.format(),
                SampleDesc: Common::DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_DEFAULT,
                ..Default::default()
            };
            let mut latest = None;
            device.CreateTexture2D(&desc, None, Some(&mut latest))?;

            let mut session = Session {
                device,
                ctx,
                dup,
                latest: latest.unwrap(),
                staging: None,
                width,
                height,
                surface,
                device_name: device_name.to_string(),
                has_image: false,
            };
            session.pump(500)?;
            Ok(session)
        }
    }

    /// Copies the newest desktop image, if one arrived, into `latest`. Waits
    /// up to `timeout_ms` only while no image has been seen yet.
    fn pump(&mut self, timeout_ms: u32) -> Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
        loop {
            let wait = if self.has_image {
                0
            } else {
                deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as u32
            };
            let mut info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut res = None;
            match unsafe { self.dup.AcquireNextFrame(wait, &mut info, &mut res) } {
                Ok(()) => {
                    let copied = self.take(&info, res);
                    unsafe { self.dup.ReleaseFrame()? };
                    copied?;
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(()),
                Err(e) => return Err(e).context("AcquireNextFrame"),
            }
            if self.has_image || std::time::Instant::now() >= deadline {
                return Ok(());
            }
        }
    }

    fn take(&mut self, info: &DXGI_OUTDUPL_FRAME_INFO, res: Option<IDXGIResource>) -> Result<()> {
        if info.LastPresentTime == 0 {
            return Ok(());
        }
        let tex: ID3D11Texture2D = res.context("frame without a surface")?.cast()?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { tex.GetDesc(&mut desc) };
        if desc.Format != self.surface.format() || desc.Width != self.width || desc.Height != self.height {
            // HDR toggled or the mode changed; the caller reopens the session.
            return Err(anyhow!("desktop surface changed to {:?} {}x{}", desc.Format, desc.Width, desc.Height));
        }
        unsafe { self.ctx.CopyResource(&self.latest, &tex) };
        self.has_image = true;
        Ok(())
    }

    fn grab(&mut self) -> Result<Bitmap> {
        self.pump(100)?;
        if !self.has_image {
            return Err(anyhow!("no desktop image from duplication yet"));
        }
        if let Surface::Hdr(map) = &self.surface {
            let white = sdr_white(&self.device_name);
            if (white - map.white()).abs() > 1e-3 {
                log::info!("SDR white changed to {:.0} nits", white * 80.0);
                self.surface = Surface::Hdr(ToneMap::new(white));
            }
        }
        let row_out = self.width as usize * 4;
        let row_in = self.width as usize * self.surface.bytes_per_pixel();
        let mut out = vec![0u8; row_out * self.height as usize];
        let staging = self.staging()?;
        unsafe {
            self.ctx.CopyResource(&staging, &self.latest);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            self.ctx.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
            for (y, dst) in out.chunks_exact_mut(row_out).enumerate() {
                let src = std::slice::from_raw_parts((m.pData as *const u8).add(y * m.RowPitch as usize), row_in);
                match &self.surface {
                    Surface::Sdr => {
                        dst.copy_from_slice(src);
                        // Duplication leaves alpha undefined; the renderer blends with it.
                        for px in dst.as_chunks_mut::<4>().0 {
                            px[3] = 255;
                        }
                    }
                    Surface::Hdr(map) => map.convert_row(src, dst),
                }
            }
            self.ctx.Unmap(&staging, 0);
        }
        Ok(Bitmap::from_bgra(self.width, self.height, out))
    }

    fn staging(&mut self) -> Result<ID3D11Texture2D> {
        if let Some(t) = &self.staging {
            return Ok(t.clone());
        }
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { self.latest.GetDesc(&mut desc) };
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        let mut staging = None;
        unsafe { self.device.CreateTexture2D(&desc, None, Some(&mut staging))? };
        Ok(self.staging.insert(staging.unwrap()).clone())
    }

    /// Releasing the staging texture alone is not enough: the driver keeps
    /// its memory pooled until the device is trimmed.
    fn trim(&mut self) {
        if self.staging.take().is_none() {
            return;
        }
        unsafe {
            self.ctx.ClearState();
            if let Ok(d) = self.device.cast::<IDXGIDevice3>() {
                d.Trim();
            }
        }
    }
}
