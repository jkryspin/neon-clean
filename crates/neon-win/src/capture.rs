//! Window pixel capture through GDI, returned as tightly packed RGB.

use anyhow::{bail, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};

/// Grab a client-area rectangle. `x`/`y` are client coordinates.
pub fn grab(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) -> Result<Vec<u8>> {
    if w <= 0 || h <= 0 {
        bail!("empty capture rect");
    }
    unsafe {
        let window_dc = GetDC(Some(hwnd));
        if window_dc.is_invalid() {
            bail!("GetDC failed");
        }
        let mem_dc = CreateCompatibleDC(Some(window_dc));
        let bitmap = CreateCompatibleBitmap(window_dc, w, h);
        let old = SelectObject(mem_dc, bitmap.into());
        let ok = BitBlt(mem_dc, 0, 0, w, h, Some(window_dc), x, y, SRCCOPY).is_ok();
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut bgra = vec![0u8; (w * h * 4) as usize];
        let lines = if ok {
            GetDIBits(
                mem_dc,
                bitmap,
                0,
                h as u32,
                Some(bgra.as_mut_ptr() as *mut _),
                &mut info,
                DIB_RGB_COLORS,
            )
        } else {
            0
        };
        let _ = SelectObject(mem_dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(mem_dc);
        ReleaseDC(Some(hwnd), window_dc);
        if lines == 0 {
            bail!("BitBlt/GetDIBits failed");
        }
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for px in bgra.chunks_exact(4) {
            rgb.extend_from_slice(&px[2..3]);
            rgb.extend_from_slice(&px[1..2]);
            rgb.extend_from_slice(&px[0..1]);
        }
        Ok(rgb)
    }
}

pub fn hash(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) -> Option<String> {
    let rgb = grab(hwnd, x, y, w, h).ok()?;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in rgb {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    Some(format!("{h:016x}"))
}

pub fn mean_gray(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) -> Option<f64> {
    let rgb = grab(hwnd, x, y, w, h).ok()?;
    if rgb.is_empty() {
        return None;
    }
    let sum: u64 = rgb
        .chunks_exact(3)
        .map(|p| (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3)
        .sum::<u32>() as u64;
    Some(sum as f64 / (rgb.len() / 3) as f64 / 255.0)
}

pub fn count_non_flat(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, tolerance: u8) -> Option<usize> {
    let rgb = grab(hwnd, x, y, w, h).ok()?;
    let first = *rgb.first()?;
    Some(
        rgb.chunks_exact(3)
            .filter(|p| p[0].abs_diff(first) > tolerance)
            .count(),
    )
}

pub fn save_bmp(hwnd: HWND, x: i32, y: i32, w: i32, h: i32, path: &std::path::Path) -> Result<()> {
    let rgb = grab(hwnd, x, y, w, h)?;
    let row = (w as usize * 3 + 3) & !3;
    let size = 54 + row * h as usize;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&(-h).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((row * h as usize) as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for y in 0..h as usize {
        let start = y * w as usize * 3;
        let mut line = rgb[start..start + w as usize * 3].to_vec();
        for px in line.chunks_exact_mut(3) {
            px.swap(0, 2);
        }
        line.resize(row, 0);
        out.extend_from_slice(&line);
    }
    std::fs::write(path, out)?;
    Ok(())
}
