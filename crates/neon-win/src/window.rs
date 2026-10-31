//! Windows lookup and window-relative input helpers.

use std::ffi::c_void;

use anyhow::{bail, Result};
use windows::core::HSTRING;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetClientRect, GetWindowRect,
    GetWindowTextW, IsWindowVisible, PostMessageW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    WM_KEYDOWN, WM_KEYUP,
};

pub struct Window {
    pub hwnd: HWND,
    pub title: String,
    pub class: String,
    pub rect: RECT,
}

pub fn find(title_contains: &str) -> Option<Window> {
    struct Ctx {
        want: String,
        hit: Option<HWND>,
    }
    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(l.0 as *mut Ctx);
        if IsWindowVisible(hwnd).as_bool() {
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n as usize]);
            if title.to_ascii_lowercase().contains(&ctx.want) {
                ctx.hit = Some(hwnd);
                return false.into();
            }
        }
        true.into()
    }
    let mut ctx = Ctx {
        want: title_contains.to_ascii_lowercase(),
        hit: None,
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.hit.map(describe)
}

pub fn find_class(class: &str) -> Option<Window> {
    unsafe {
        let w = FindWindowW(&HSTRING::from(class), None).ok()?;
        Some(describe(w))
    }
}

pub fn child_windows(parent: HWND, class: &str) -> Vec<Window> {
    let mut out = Vec::new();
    let mut prev = HWND::default();
    loop {
        let Ok(w) =
            (unsafe { FindWindowExW(Some(parent), Some(prev), &HSTRING::from(class), None) })
        else {
            break;
        };
        out.push(describe(w));
        prev = w;
    }
    out
}

pub fn describe(hwnd: HWND) -> Window {
    let mut title_buf = [0u16; 512];
    let mut class_buf = [0u16; 256];
    unsafe {
        let n = GetWindowTextW(hwnd, &mut title_buf);
        let c = GetClassNameW(hwnd, &mut class_buf);
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        Window {
            hwnd,
            title: String::from_utf16_lossy(&title_buf[..n as usize]),
            class: String::from_utf16_lossy(&class_buf[..c as usize]),
            rect,
        }
    }
}

pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut r = RECT::default();
    unsafe {
        if GetClientRect(hwnd, &mut r).is_ok() {
            (r.right - r.left, r.bottom - r.top)
        } else {
            (0, 0)
        }
    }
}

pub fn focus(hwnd: HWND) -> Result<()> {
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        if !SetForegroundWindow(hwnd).as_bool() {
            bail!("SetForegroundWindow refused");
        }
    }
    Ok(())
}

pub fn click(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    let lp = ((y as isize) << 16) | (x as isize & 0xFFFF);
    unsafe {
        PostMessageW(
            Some(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE,
            Default::default(),
            windows::Win32::Foundation::LPARAM(lp),
        )?;
        PostMessageW(
            Some(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONDOWN,
            windows::Win32::Foundation::WPARAM(1),
            windows::Win32::Foundation::LPARAM(lp),
        )?;
        PostMessageW(
            Some(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_LBUTTONUP,
            Default::default(),
            windows::Win32::Foundation::LPARAM(lp),
        )?;
    }
    Ok(())
}

pub fn post_key(hwnd: HWND, vk: u16, down: bool, scan: u16) {
    let msg = if down { WM_KEYDOWN } else { WM_KEYUP };
    let lp = 1 | ((scan as isize) << 16) | if down { 0 } else { (1 << 30) | (1 << 31) };
    unsafe {
        let _ = PostMessageW(
            Some(hwnd),
            msg,
            windows::Win32::Foundation::WPARAM(vk as usize),
            LPARAM(lp),
        );
    }
}

pub fn has_dialog(pid: u32) -> bool {
    struct Ctx {
        pid: u32,
        hit: bool,
    }
    unsafe extern "system" fn cb(hwnd: HWND, l: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(l.0 as *mut Ctx);
        let mut p = 0u32;
        unsafe {
            windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(hwnd, Some(&mut p));
        }
        if p == ctx.pid {
            let mut buf = [0u16; 64];
            let n = unsafe { GetClassNameW(hwnd, &mut buf) };
            if String::from_utf16_lossy(&buf[..n as usize]) == "#32770" {
                ctx.hit = true;
                return false.into();
            }
        }
        true.into()
    }
    let mut ctx = Ctx { pid, hit: false };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.hit
}

#[allow(dead_code)]
fn _void(_: *mut c_void) {}

pub fn mouse_move(hwnd: HWND, x: i32, y: i32) -> Result<()> {
    let lp = ((y as isize) << 16) | (x as isize & 0xFFFF);
    unsafe {
        PostMessageW(
            Some(hwnd),
            windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE,
            Default::default(),
            windows::Win32::Foundation::LPARAM(lp),
        )?;
    }
    Ok(())
}
