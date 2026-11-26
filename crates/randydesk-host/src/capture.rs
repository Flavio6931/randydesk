//! Captura de pantalla de baja latencia mediante DXGI Desktop Duplication con fallback a GDI.

use anyhow::{anyhow, Context, Result};
use randydesk_proto::{CursorKind, MonitorInfo};
use std::mem::size_of;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tracing::{info, warn};
use windows::core::Interface;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIOutput, IDXGIOutput1, IDXGIOutputDuplication, IDXGIResource,
    DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT, DXGI_OUTDUPL_FRAME_INFO,
};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EnumDisplayMonitors,
    GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS, HBITMAP, HDC, HMONITOR, SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorInfo, GetSystemMetrics, LoadCursorW, CURSORINFO, CURSOR_SHOWING, IDC_APPSTARTING,
    IDC_CROSS, IDC_HAND, IDC_IBEAM, IDC_NO, IDC_SIZEALL, IDC_SIZENESW, IDC_SIZENS, IDC_SIZENWSE,
    IDC_SIZEWE, IDC_WAIT, SM_CXSCREEN, SM_CYSCREEN,
};

/// Geometría en coordenadas del escritorio virtual del monitor capturado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorGeometry {
    /// Origen X en píxeles dentro del escritorio virtual.
    pub origin_x: i32,
    /// Origen Y en píxeles dentro del escritorio virtual.
    pub origin_y: i32,
    /// Ancho del monitor en píxeles.
    pub width: u32,
    /// Alto del monitor en píxeles.
    pub height: u32,
}

/// Cuadro capturado en formato BGRA8 empaquetado (4 bytes por píxel).
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    /// Ancho del cuadro en píxeles.
    pub width: u32,
    /// Alto del cuadro en píxeles.
    pub height: u32,
    /// Buffer continuo de tamaño `width * height * 4` en orden B, G, R, A.
    pub bgra: Vec<u8>,
}

/// Interfaz unificada para backends de captura de pantalla.
pub trait ScreenCapturer: Send {
    /// Captura el siguiente cuadro disponible; devuelve `Ok(None)` si no hubo cambios dentro de `timeout_ms`.
    fn capture(&mut self, timeout_ms: u32) -> Result<Option<CapturedFrame>>;

    /// Devuelve el buffer de un cuadro ya procesado para reutilizar su capacidad sin realocar memoria.
    fn recycle_buffer(&mut self, _bgra: Vec<u8>) {}

    /// Devuelve la geometría actual del monitor capturado.
    fn dimensions(&self) -> MonitorGeometry;
}

/// Crea el mejor capturador disponible para `monitor_index`: intenta DXGI y cae a GDI si falla.
pub fn create_capturer(monitor_index: usize) -> Result<Box<dyn ScreenCapturer>> {
    let _ = crate::input::sync_thread_to_input_desktop();
    match DxgiCapturer::new(monitor_index) {
        Ok(dxgi) => {
            let dims = dxgi.dimensions();
            info!(
                "Captura iniciada con DXGI Desktop Duplication (monitor {}, {}x{} en {},{})",
                monitor_index, dims.width, dims.height, dims.origin_x, dims.origin_y
            );
            Ok(Box::new(dxgi))
        }
        Err(err) => {
            warn!(
                "DXGI no disponible ({err:#}); activando fallback automático a GDI"
            );
            let gdi = GdiCapturer::new(monitor_index)?;
            let dims = gdi.dimensions();
            info!(
                "Captura iniciada con GDI (monitor {}, {}x{} en {},{})",
                monitor_index, dims.width, dims.height, dims.origin_x, dims.origin_y
            );
            Ok(Box::new(gdi))
        }
    }
}

/// Guardia RAII que garantiza llamar a `ReleaseFrame` al salir del ámbito de un frame DXGI.
struct DxgiFrameGuard {
    dup: IDXGIOutputDuplication,
}

impl Drop for DxgiFrameGuard {
    fn drop(&mut self) {
        // Ignoramos errores al liberar si el dispositivo ya se perdió.
        unsafe {
            let _ = self.dup.ReleaseFrame();
        }
    }
}

/// Límite máximo de cuadros acumulados a drenar por ciclo para evitar inanición en tasas altas.
const MAX_DRAIN_FRAMES: usize = 8;

/// Capturador de alto rendimiento basado en DXGI Desktop Duplication API.
pub struct DxgiCapturer {
    monitor_index: usize,
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    output1: IDXGIOutput1,
    dup: IDXGIOutputDuplication,
    staging: Option<ID3D11Texture2D>,
    bgra: Vec<u8>,
    geometry: MonitorGeometry,
    gdi_fallback: Option<GdiCapturer>,
    last_dxgi_retry: Instant,
}

impl DxgiCapturer {
    /// Inicializa el dispositivo D3D11 y la duplicación del monitor especificado.
    pub fn new(monitor_index: usize) -> Result<Self> {
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&[D3D_FEATURE_LEVEL_11_0]),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("falló D3D11CreateDevice")?;
        }
        let device = device.ok_or_else(|| anyhow!("D3D11CreateDevice devolvió device nulo"))?;
        let context = context.ok_or_else(|| anyhow!("D3D11CreateDevice devolvió context nulo"))?;

        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let output: IDXGIOutput = unsafe {
            adapter
                .EnumOutputs(monitor_index as u32)
                .with_context(|| format!("monitor índice {monitor_index} no encontrado en DXGI"))?
        };
        let desc = unsafe { output.GetDesc()? };
        let rect = desc.DesktopCoordinates;
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        let geometry = MonitorGeometry {
            origin_x: rect.left,
            origin_y: rect.top,
            width,
            height,
        };

        let output1: IDXGIOutput1 = output.cast()?;
        let dup = unsafe {
            output1
                .DuplicateOutput(&device)
                .context("falló DuplicateOutput (posible sesión sin composición o UAC)")?
        };

        Ok(Self {
            monitor_index,
            device,
            context,
            output1,
            dup,
            staging: None,
            bgra: Vec::new(),
            geometry,
            gdi_fallback: None,
            last_dxgi_retry: Instant::now(),
        })
    }

    fn recreate_duplication(&mut self) -> Result<()> {
        let _ = crate::input::sync_thread_to_input_desktop();
        self.staging = None;
        let dxgi_device: IDXGIDevice = self.device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let output: IDXGIOutput = unsafe { adapter.EnumOutputs(self.monitor_index as u32)? };
        let desc = unsafe { output.GetDesc()? };
        let rect = desc.DesktopCoordinates;
        self.geometry = MonitorGeometry {
            origin_x: rect.left,
            origin_y: rect.top,
            width: (rect.right - rect.left).max(1) as u32,
            height: (rect.bottom - rect.top).max(1) as u32,
        };
        self.output1 = output.cast()?;
        self.dup = unsafe { self.output1.DuplicateOutput(&self.device)? };
        Ok(())
    }

    fn ensure_staging(&mut self, width: u32, height: u32) -> Result<ID3D11Texture2D> {
        if let Some(tex) = &self.staging {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe { tex.GetDesc(&mut desc) };
            if desc.Width == width && desc.Height == height {
                return Ok(tex.clone());
            }
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut tex: Option<ID3D11Texture2D> = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut tex))
                .context("falló CreateTexture2D staging")?;
        }
        let tex = tex.ok_or_else(|| anyhow!("CreateTexture2D devolvió textura nula"))?;
        self.staging = Some(tex.clone());
        Ok(tex)
    }

    fn copy_resource_to_staging(
        &mut self,
        resource: Option<IDXGIResource>,
    ) -> Result<(ID3D11Texture2D, u32, u32)> {
        let resource = resource.ok_or_else(|| anyhow!("AcquireNextFrame devolvió recurso nulo"))?;
        let gpu_tex: ID3D11Texture2D = resource.cast()?;
        let mut gpu_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { gpu_tex.GetDesc(&mut gpu_desc) };

        let width = gpu_desc.Width;
        let height = gpu_desc.Height;
        self.geometry.width = width;
        self.geometry.height = height;

        let staging = self.ensure_staging(width, height)?;
        unsafe {
            self.context.CopyResource(&staging, &gpu_tex);
        }
        Ok((staging, width, height))
    }
}

impl ScreenCapturer for DxgiCapturer {
    fn capture(&mut self, timeout_ms: u32) -> Result<Option<CapturedFrame>> {
        if self.gdi_fallback.is_some() {
            if self.last_dxgi_retry.elapsed() >= Duration::from_secs(1) {
                self.last_dxgi_retry = Instant::now();
                if self.recreate_duplication().is_ok() {
                    info!("DXGI Desktop Duplication recuperado tras escritorio seguro");
                    self.gdi_fallback = None;
                } else if let Some(gdi) = self.gdi_fallback.as_mut() {
                    return gdi.capture(timeout_ms);
                }
            } else if let Some(gdi) = self.gdi_fallback.as_mut() {
                return gdi.capture(timeout_ms);
            }
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut resource: Option<IDXGIResource> = None;

        match unsafe { self.dup.AcquireNextFrame(timeout_ms, &mut frame_info, &mut resource) } {
            Ok(()) => {}
            Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                warn!("DXGI_ERROR_ACCESS_LOST detectado; recreando duplicación");
                if self.recreate_duplication().is_err() {
                    let mut gdi = GdiCapturer::new(self.monitor_index)?;
                    let frame = gdi.capture(timeout_ms);
                    self.gdi_fallback = Some(gdi);
                    self.last_dxgi_retry = Instant::now();
                    return frame;
                }
                return Ok(None);
            }
            Err(e) => return Err(e).context("error en AcquireNextFrame"),
        }

        let (mut staging, mut width, mut height) = {
            let _guard = DxgiFrameGuard {
                dup: self.dup.clone(),
            };
            self.copy_resource_to_staging(resource)?
        };

        // Drenar cuadros acumulados para copiar únicamente el más reciente a memoria CPU.
        for _ in 0..MAX_DRAIN_FRAMES {
            let mut drained_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut drained_res: Option<IDXGIResource> = None;
            match unsafe { self.dup.AcquireNextFrame(0, &mut drained_info, &mut drained_res) } {
                Ok(()) => {
                    let _guard = DxgiFrameGuard {
                        dup: self.dup.clone(),
                    };
                    let (next_staging, next_w, next_h) = self.copy_resource_to_staging(drained_res)?;
                    staging = next_staging;
                    width = next_w;
                    height = next_h;
                }
                Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => break,
                Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                    warn!("DXGI_ERROR_ACCESS_LOST detectado al drenar; recreando duplicación");
                    self.recreate_duplication()?;
                    return Ok(None);
                }
                Err(e) => return Err(e).context("error drenando AcquireNextFrame(0)"),
            }
        }

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe {
            self.context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .context("falló Map de textura staging")?;
        }

        let row_bytes = (width as usize) * 4;
        let pitch = mapped.RowPitch as usize;
        let needed_bytes = row_bytes * (height as usize);
        let mut bgra = std::mem::take(&mut self.bgra);
        bgra.resize(needed_bytes, 0);

        unsafe {
            let src_ptr = mapped.pData as *const u8;
            for y in 0..(height as usize) {
                let src_row = std::slice::from_raw_parts(src_ptr.add(y * pitch), row_bytes);
                let dst_row = &mut bgra[y * row_bytes..(y + 1) * row_bytes];
                dst_row.copy_from_slice(src_row);
            }
            self.context.Unmap(&staging, 0);
        }

        Ok(Some(CapturedFrame {
            width,
            height,
            bgra,
        }))
    }

    fn recycle_buffer(&mut self, bgra: Vec<u8>) {
        if bgra.capacity() >= self.bgra.capacity() {
            self.bgra = bgra;
        }
    }

    fn dimensions(&self) -> MonitorGeometry {
        self.geometry
    }
}

/// Capturador de respaldo basado en Win32 GDI (`BitBlt`) para entornos sin soporte DXGI.
pub struct GdiCapturer {
    geometry: MonitorGeometry,
}

impl GdiCapturer {
    /// Inicializa la geometría del monitor usando `EnumDisplayMonitors` o pantalla primaria.
    pub fn new(monitor_index: usize) -> Result<Self> {
        let monitors = enumerate_monitor_rects();
        let geometry = if let Some(rect) = monitors.get(monitor_index) {
            MonitorGeometry {
                origin_x: rect.left,
                origin_y: rect.top,
                width: (rect.right - rect.left).max(1) as u32,
                height: (rect.bottom - rect.top).max(1) as u32,
            }
        } else {
            let width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1) as u32;
            let height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1) as u32;
            MonitorGeometry {
                origin_x: 0,
                origin_y: 0,
                width,
                height,
            }
        };
        Ok(Self { geometry })
    }
}

impl ScreenCapturer for GdiCapturer {
    fn capture(&mut self, _timeout_ms: u32) -> Result<Option<CapturedFrame>> {
        let _ = crate::input::sync_thread_to_input_desktop();
        let width = self.geometry.width;
        let height = self.geometry.height;

        unsafe {
            let screen_dc = GetDC(None);
            if screen_dc.is_invalid() {
                return Err(anyhow!("GetDC(None) falló"));
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.is_invalid() {
                ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateCompatibleDC falló"));
            }
            let bmp: HBITMAP = CreateCompatibleBitmap(screen_dc, width as i32, height as i32);
            if bmp.is_invalid() {
                let _ = DeleteDC(mem_dc);
                ReleaseDC(None, screen_dc);
                return Err(anyhow!("CreateCompatibleBitmap falló"));
            }

            let old_obj = SelectObject(mem_dc, bmp);
            let blt_res = BitBlt(
                mem_dc,
                0,
                0,
                width as i32,
                height as i32,
                screen_dc,
                self.geometry.origin_x,
                self.geometry.origin_y,
                SRCCOPY,
            );

            let mut bgra = vec![0u8; (width as usize) * (height as usize) * 4];
            if blt_res.is_ok() {
                let mut bi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width as i32,
                        biHeight: -(height as i32), // top-down
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let scanlines = GetDIBits(
                    mem_dc,
                    bmp,
                    0,
                    height,
                    Some(bgra.as_mut_ptr() as *mut _),
                    &mut bi,
                    DIB_RGB_COLORS,
                );
                if scanlines == 0 {
                    SelectObject(mem_dc, old_obj);
                    let _ = DeleteObject(bmp);
                    let _ = DeleteDC(mem_dc);
                    ReleaseDC(None, screen_dc);
                    return Err(anyhow!("GetDIBits devolvió 0 scanlines"));
                }
            }

            SelectObject(mem_dc, old_obj);
            let _ = DeleteObject(bmp);
            let _ = DeleteDC(mem_dc);
            ReleaseDC(None, screen_dc);
            blt_res.context("BitBlt falló")?;

            Ok(Some(CapturedFrame {
                width,
                height,
                bgra,
            }))
        }
    }

    fn dimensions(&self) -> MonitorGeometry {
        self.geometry
    }
}

unsafe extern "system" fn monitor_enum_proc(
    _hmonitor: HMONITOR,
    _hdc: HDC,
    lprect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    if !lprect.is_null() {
        let list = &mut *(lparam.0 as *mut Vec<RECT>);
        list.push(*lprect);
    }
    BOOL(1)
}

fn enumerate_monitor_rects() -> Vec<RECT> {
    let mut rects: Vec<RECT> = Vec::new();
    unsafe {
        let ptr = (&mut rects as *mut Vec<RECT>) as isize;
        let _ = EnumDisplayMonitors(None, None, Some(monitor_enum_proc), LPARAM(ptr));
    }
    rects
}

fn list_dxgi_monitors() -> Result<Vec<MonitorInfo>> {
    let mut device: Option<ID3D11Device> = None;
    let mut context: Option<ID3D11DeviceContext> = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.ok_or_else(|| anyhow!("D3D11CreateDevice nulo"))?;
    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };

    let mut monitors = Vec::new();
    for idx in 0u32..16u32 {
        let Ok(output) = (unsafe { adapter.EnumOutputs(idx) }) else {
            break;
        };
        let Ok(desc) = (unsafe { output.GetDesc() }) else {
            continue;
        };
        let rect = desc.DesktopCoordinates;
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        monitors.push(MonitorInfo {
            index: idx as u8,
            name: format!("Monitor {}", idx + 1),
            width,
            height,
            primary: rect.left == 0 && rect.top == 0,
        });
    }
    Ok(monitors)
}

/// Enumera los monitores activos en el sistema coincidiendo 1:1 con el índice DXGI/GDI.
pub fn list_monitors() -> Vec<MonitorInfo> {
    if let Ok(dxgi_list) = list_dxgi_monitors() {
        if !dxgi_list.is_empty() {
            return dxgi_list;
        }
    }
    let rects = enumerate_monitor_rects();
    if !rects.is_empty() {
        return rects
            .into_iter()
            .enumerate()
            .map(|(idx, r)| MonitorInfo {
                index: idx as u8,
                name: format!("Monitor {}", idx + 1),
                width: (r.right - r.left).max(1) as u32,
                height: (r.bottom - r.top).max(1) as u32,
                primary: r.left == 0 && r.top == 0,
            })
            .collect();
    }
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1) as u32;
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1) as u32;
    vec![MonitorInfo {
        index: 0,
        name: "Monitor 1".to_string(),
        width,
        height,
        primary: true,
    }]
}

struct SystemCursors {
    ibeam: usize,
    hand: usize,
    cross: usize,
    size_all: usize,
    size_we: usize,
    size_ns: usize,
    size_nesw: usize,
    size_nwse: usize,
    no: usize,
    wait: usize,
    app_starting: usize,
}

fn system_cursors() -> &'static SystemCursors {
    static CURSORS: OnceLock<SystemCursors> = OnceLock::new();
    CURSORS.get_or_init(|| unsafe {
        let load = |id| -> usize {
            LoadCursorW(None, id)
                .map(|h| h.0 as usize)
                .unwrap_or(0)
        };
        SystemCursors {
            ibeam: load(IDC_IBEAM),
            hand: load(IDC_HAND),
            cross: load(IDC_CROSS),
            size_all: load(IDC_SIZEALL),
            size_we: load(IDC_SIZEWE),
            size_ns: load(IDC_SIZENS),
            size_nesw: load(IDC_SIZENESW),
            size_nwse: load(IDC_SIZENWSE),
            no: load(IDC_NO),
            wait: load(IDC_WAIT),
            app_starting: load(IDC_APPSTARTING),
        }
    })
}

/// Consulta la forma estándar del cursor activo de Windows para replicarla en el visor remoto.
pub fn query_system_cursor() -> CursorKind {
    let mut info = CURSORINFO {
        cbSize: size_of::<CURSORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetCursorInfo(&mut info) }.is_err() {
        return CursorKind::Default;
    }
    if (info.flags.0 & CURSOR_SHOWING.0) == 0 {
        return CursorKind::Hidden;
    }
    let h = info.hCursor.0 as usize;
    if h == 0 {
        return CursorKind::Default;
    }
    let sys = system_cursors();
    if h == sys.ibeam {
        CursorKind::Text
    } else if h == sys.hand {
        CursorKind::PointingHand
    } else if h == sys.cross {
        CursorKind::Crosshair
    } else if h == sys.size_all {
        CursorKind::Move
    } else if h == sys.size_we {
        CursorKind::ResizeHorizontal
    } else if h == sys.size_ns {
        CursorKind::ResizeVertical
    } else if h == sys.size_nesw {
        CursorKind::ResizeNeSw
    } else if h == sys.size_nwse {
        CursorKind::ResizeNwSe
    } else if h == sys.no {
        CursorKind::NotAllowed
    } else if h == sys.wait {
        CursorKind::Wait
    } else if h == sys.app_starting {
        CursorKind::Progress
    } else {
        CursorKind::Default
    }
}
