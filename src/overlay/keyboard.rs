use std::sync::atomic::{AtomicIsize, Ordering};

use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_C, VK_CONTROL, VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN,
};

pub const WM_APP_OVERLAY_KEY: u32 = WM_APP + 0x21;

static TARGET: AtomicIsize = AtomicIsize::new(0);

pub struct KeyboardHook {
    handle: HHOOK,
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.handle);
        }
        TARGET.store(0, Ordering::SeqCst);
    }
}

pub fn install(target: HWND) -> Result<KeyboardHook> {
    TARGET.store(target.0 as isize, Ordering::SeqCst);
    let handle = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)? };
    Ok(KeyboardHook { handle })
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 {
        let keyboard = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let message = wparam.0 as u32;
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            let vk = keyboard.vkCode;
            let ctrl = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
            let handled = vk == VK_ESCAPE.0 as u32
                || vk == VK_RETURN.0 as u32
                || vk == VK_SPACE.0 as u32
                || (vk == VK_C.0 as u32 && ctrl);
            if handled {
                let target = TARGET.load(Ordering::SeqCst);
                if target != 0 {
                    let _ = PostMessageW(
                        Some(HWND(target as *mut core::ffi::c_void)),
                        WM_APP_OVERLAY_KEY,
                        WPARAM(vk as usize),
                        LPARAM(if ctrl { 1 } else { 0 }),
                    );
                }
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}
