//! Inyección de eventos de ratón y teclado en Windows mediante `SendInput`.

use crate::capture::MonitorGeometry;
use randydesk_proto::{InputEvent, MouseButton};
use std::mem::size_of;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEINPUT, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL,
    MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP,
    MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK,
    MOUSEEVENTF_WHEEL, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

/// Inyecta un evento remoto de ratón o teclado sobre la geometría del monitor capturado.
pub fn inject_input(event: &InputEvent, monitor: MonitorGeometry) {
    match *event {
        InputEvent::MouseMove { x_norm, y_norm } => {
            let (abs_x, abs_y) = map_normalized_to_virtual_desk(x_norm, y_norm, monitor);
            send_mouse(MOUSEINPUT {
                dx: abs_x,
                dy: abs_y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            });
        }
        InputEvent::MouseButton { button, down } => {
            let flag = match (button, down) {
                (MouseButton::Left, true) => MOUSEEVENTF_LEFTDOWN,
                (MouseButton::Left, false) => MOUSEEVENTF_LEFTUP,
                (MouseButton::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                (MouseButton::Right, false) => MOUSEEVENTF_RIGHTUP,
                (MouseButton::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                (MouseButton::Middle, false) => MOUSEEVENTF_MIDDLEUP,
            };
            send_mouse(MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flag,
                time: 0,
                dwExtraInfo: 0,
            });
        }
        InputEvent::MouseWheel { delta_x, delta_y } => {
            if delta_y != 0 {
                send_mouse(MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (delta_y as i32) as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                });
            }
            if delta_x != 0 {
                send_mouse(MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: (delta_x as i32) as u32,
                    dwFlags: MOUSEEVENTF_HWHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                });
            }
        }
        InputEvent::Key {
            vk,
            scan,
            extended,
            down,
        } => {
            let mut flags = windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);
            if extended {
                flags |= KEYEVENTF_EXTENDEDKEY;
            }
            if !down {
                flags |= KEYEVENTF_KEYUP;
            }
            send_keyboard(KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            });
        }
        InputEvent::UnicodeChar { ch, down } => {
            let mut buf = [0u16; 2];
            for code_unit in ch.encode_utf16(&mut buf).iter().copied() {
                let mut flags = KEYEVENTF_UNICODE;
                if !down {
                    flags |= KEYEVENTF_KEYUP;
                }
                send_keyboard(KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: code_unit,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                });
            }
        }
    }
}

fn map_normalized_to_virtual_desk(
    x_norm: f32,
    y_norm: f32,
    monitor: MonitorGeometry,
) -> (i32, i32) {
    let x_clamped = x_norm.clamp(0.0, 1.0);
    let y_clamped = y_norm.clamp(0.0, 1.0);

    let target_x = monitor.origin_x as f64 + (x_clamped as f64) * (monitor.width.saturating_sub(1) as f64);
    let target_y = monitor.origin_y as f64 + (y_clamped as f64) * (monitor.height.saturating_sub(1) as f64);

    let virt_x = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) } as f64;
    let virt_y = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) } as f64;
    let virt_w = unsafe { GetSystemMetrics(SM_CXVIRTUALSCREEN) }.max(1) as f64;
    let virt_h = unsafe { GetSystemMetrics(SM_CYVIRTUALSCREEN) }.max(1) as f64;

    let abs_x = (((target_x - virt_x) * 65535.0) / (virt_w - 1.0).max(1.0)).round() as i32;
    let abs_y = (((target_y - virt_y) * 65535.0) / (virt_h - 1.0).max(1.0)).round() as i32;
    (abs_x.clamp(0, 65535), abs_y.clamp(0, 65535))
}

fn send_mouse(mi: MOUSEINPUT) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi },
    };
    unsafe {
        let _ = SendInput(&[input], size_of::<INPUT>() as i32);
    }
}

fn send_keyboard(ki: KEYBDINPUT) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki },
    };
    unsafe {
        let _ = SendInput(&[input], size_of::<INPUT>() as i32);
    }
}
