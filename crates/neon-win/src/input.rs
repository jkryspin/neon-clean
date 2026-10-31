//! Global keyboard and mouse input through SendInput.

use std::thread;
use std::time::Duration;

use anyhow::{bail, Result};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MAPVK_VK_TO_VSC, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEINPUT, VIRTUAL_KEY,
};

fn key_input(vk: u16, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: if up {
                    KEYEVENTF_KEYUP | KEYEVENTF_SCANCODE
                } else {
                    KEYEVENTF_SCANCODE
                } as KEYBD_EVENT_FLAGS,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn key(vk: u16, down: bool) -> Result<()> {
    let input = key_input(vk, !down);
    let n = unsafe {
        SendInput(
            std::slice::from_ref(&input),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if n == 0 {
        bail!("SendInput(key) failed");
    }
    Ok(())
}

pub fn tap(vk: u16, hold: Duration) -> Result<()> {
    key(vk, true)?;
    thread::sleep(hold);
    key(vk, false)
}

fn mouse(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> Result<()> {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let n = unsafe {
        SendInput(
            std::slice::from_ref(&input),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if n == 0 {
        bail!("SendInput(mouse) failed");
    }
    Ok(())
}

pub fn click(right: bool) -> Result<()> {
    if right {
        mouse(MOUSEEVENTF_RIGHTDOWN)?;
        thread::sleep(Duration::from_millis(25));
        mouse(MOUSEEVENTF_RIGHTUP)
    } else {
        mouse(MOUSEEVENTF_LEFTDOWN)?;
        thread::sleep(Duration::from_millis(25));
        mouse(MOUSEEVENTF_LEFTUP)
    }
}

pub fn type_text(text: &str) -> Result<()> {
    for ch in text.encode_utf16() {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: ch,
                    dwFlags: KEYEVENTF_SCANCODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let mut up = input;
        up.Anonymous.ki.dwFlags = KEYEVENTF_KEYUP | KEYEVENTF_SCANCODE;
        unsafe {
            SendInput(
                std::slice::from_ref(&input),
                std::mem::size_of::<INPUT>() as i32,
            );
            SendInput(
                std::slice::from_ref(&up),
                std::mem::size_of::<INPUT>() as i32,
            );
        }
        thread::sleep(Duration::from_millis(8));
    }
    Ok(())
}

pub fn is_down(vk: u16) -> bool {
    unsafe {
        windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(vk as i32) as u16 & 0x8000
            != 0
    }
}

pub fn mouse_to(x: i32, y: i32) -> Result<()> {
    use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;
    unsafe { SetCursorPos(x, y).map_err(|e| anyhow::anyhow!(e.to_string())) }
}

pub fn mouse_pos() -> Result<(i32, i32)> {
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = windows::Win32::Foundation::POINT::default();
    unsafe { GetCursorPos(&mut p).map_err(|e| anyhow::anyhow!(e.to_string()))? };
    Ok((p.x, p.y))
}
