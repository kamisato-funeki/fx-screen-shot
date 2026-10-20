use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use anyhow::Result;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_C, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RETURN, VK_RWIN, VK_SPACE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, PostMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use crate::win_util;

use super::{KEY_MOD_ALT, KEY_MOD_CTRL, KEY_MOD_WIN};

pub const WM_APP_OVERLAY_KEY: u32 = WM_APP + 0x21;

static TARGET: AtomicIsize = AtomicIsize::new(0);
static C_DOWN: AtomicBool = AtomicBool::new(false);

pub fn current_modifiers() -> u32 {
    let mut modifiers = 0;
    if win_util::key_down(VK_CONTROL.0 as u32) {
        modifiers |= KEY_MOD_CTRL;
    }
    if win_util::key_down(VK_MENU.0 as u32) {
        modifiers |= KEY_MOD_ALT;
    }
    if win_util::key_down(VK_LWIN.0 as u32) || win_util::key_down(VK_RWIN.0 as u32) {
        modifiers |= KEY_MOD_WIN;
    }
    modifiers
}

pub struct KeyboardHook {
    handle: HHOOK,
}

impl Drop for KeyboardHook {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.handle);
        }
        TARGET.store(0, Ordering::SeqCst);
        C_DOWN.store(false, Ordering::SeqCst);
    }
}

pub fn install(target: HWND) -> Result<KeyboardHook> {
    TARGET.store(target.0 as isize, Ordering::SeqCst);
    C_DOWN.store(false, Ordering::SeqCst);
    let handle = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)? };
    Ok(KeyboardHook { handle })
}

unsafe extern "system" fn hook_proc(ncode: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if ncode == HC_ACTION as i32 {
        let keyboard = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let message = wparam.0 as u32;
        let vk = keyboard.vkCode;
        if message == WM_KEYUP || message == WM_SYSKEYUP {
            if vk == VK_C.0 as u32 {
                C_DOWN.store(false, Ordering::SeqCst);
            }
            return CallNextHookEx(None, ncode, wparam, lparam);
        }
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            let modifiers = current_modifiers();
            let is_c = vk == VK_C.0 as u32;
            // Plain C copies the color under the cursor; Ctrl+C copies the
            // selected image. Alt/Win combinations stay with the system.
            let handled = if is_c {
                modifiers & (KEY_MOD_ALT | KEY_MOD_WIN) == 0
            } else {
                vk == VK_ESCAPE.0 as u32 || vk == VK_RETURN.0 as u32 || vk == VK_SPACE.0 as u32
            };
            if handled {
                if is_c && C_DOWN.swap(true, Ordering::SeqCst) {
                    return LRESULT(1);
                }
                let target = TARGET.load(Ordering::SeqCst);
                if target != 0 {
                    let _ = PostMessageW(
                        Some(HWND(target as *mut core::ffi::c_void)),
                        WM_APP_OVERLAY_KEY,
                        WPARAM(vk as usize),
                        LPARAM(modifiers as isize),
                    );
                }
                return LRESULT(1);
            }
        }
    }
    CallNextHookEx(None, ncode, wparam, lparam)
}
