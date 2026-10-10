//! Windows Terminal's zoom keys, for a zoom per screen
//! (`crate::terminal_zoom`): the terminal keeps them, so litastum watches
//! them with a low-level keyboard hook -- every press, each auto-repeat
//! too, and its own presses told apart by Windows (`LLKHF_INJECTED`) --
//! and presses them itself to move the terminal's font size. Polling the
//! keys (as `Ctrl+V` is) missed quick presses and could only guess the
//! repeats, and the count drifted.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState,
    SendInput,
    INPUT,
    INPUT_0,
    INPUT_KEYBOARD,
    KEYBDINPUT,
    KEYEVENTF_KEYUP,
    VIRTUAL_KEY,
    VK_ADD,
    VK_CONTROL,
    VK_MENU,
    VK_NUMPAD0,
    VK_OEM_MINUS,
    VK_OEM_PLUS,
    VK_SUBTRACT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx,
    GetClassNameW,
    GetForegroundWindow,
    GetMessageW,
    SetWindowsHookExW,
    KBDLLHOOKSTRUCT,
    LLKHF_INJECTED,
    MSG,
    WH_KEYBOARD_LL,
    WM_KEYDOWN,
    WM_SYSKEYDOWN,
};

use crate::terminal_zoom::TerminalZoom;

/// `'0'` is its own ASCII value as a virtual key.
const VK_0: u32 = b'0' as u32;

/// Windows Terminal's window class: litastum presses keys only into it.
const TERMINAL_WINDOW_CLASS: &str = "CASCADIA_HOSTING_WINDOW_CLASS";

/// Presses the user made since last taken: `+1` per `Ctrl`+`+`, `-1` per
/// `Ctrl`+`-`.
static STEPS: AtomicI32 = AtomicI32::new(0);
/// `Ctrl`+`0` pressed since last taken.
static RESET: AtomicBool = AtomicBool::new(false);
/// Whether litastum's tab has the focus: only then are presses its own
/// (every litastum in Windows Terminal's tabs sees every key).
static FOCUSED: AtomicBool = AtomicBool::new(true);


/// A zoom per screen, when litastum runs in Windows Terminal (`WT_SESSION`)
/// -- not in its own window, which zooms per screen itself and may have
/// been started from Windows Terminal. The terminal's tab may still be
/// zoomed by an earlier litastum in it that didn't put it back.
pub fn start() -> Option<TerminalZoom> {
    if cfg!(test) || crate::image_host::host_cell_size().is_some() {
        return None;
    }
    let session = std::env::var("WT_SESSION").ok()?;
    let (steps, now) = crate::theming::config::load_terminal_zoom();
    let applied = now.filter(|now| now.session == session).map_or(0, |now| now.steps);
    std::thread::Builder::new().name("zoom-keys".into()).spawn(watch_keys).ok()?;
    Some(TerminalZoom::new(steps, applied))
}


/// This litastum's `WT_SESSION`, for saving where its tab is.
pub fn session() -> String {
    std::env::var("WT_SESSION").unwrap_or_default()
}


/// Whether litastum's tab has the focus now (`App::terminal_focused`).
pub fn set_focused(focused: bool) {
    FOCUSED.store(focused, Ordering::Relaxed);
}


/// The user's presses since last asked: the steps, and whether `Ctrl`+`0`
/// came (first).
pub fn take_presses() -> (i32, bool) {
    (STEPS.swap(0, Ordering::Relaxed), RESET.swap(false, Ordering::Relaxed))
}


/// The hook's thread: a low-level hook runs on the thread that set it,
/// which must pump messages.
fn watch_keys() {
    // SAFETY: a plain function as the hook, this module's own handle;
    // the loop below only waits for messages.
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_key), GetModuleHandleW(std::ptr::null()), 0);
        if hook.is_null() {
            tracing::warn!("zoom keys: the keyboard hook failed");
            return;
        }
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {}
    }
}


/// Counts `Ctrl`+`+`/`-`/`0` the user presses while litastum's tab has
/// the focus; lets every key through.
unsafe extern "system" fn on_key(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN) && FOCUSED.load(Ordering::Relaxed) {
        // SAFETY: for a keyboard hook `lparam` points at a KBDLLHOOKSTRUCT.
        let key = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if key.flags & LLKHF_INJECTED == 0 && down(VK_CONTROL) && !down(VK_MENU) {
            match key.vkCode {
                vk if vk == u32::from(VK_OEM_PLUS) || vk == u32::from(VK_ADD) => {
                    STEPS.fetch_add(1, Ordering::Relaxed);
                }
                vk if vk == u32::from(VK_OEM_MINUS) || vk == u32::from(VK_SUBTRACT) => {
                    STEPS.fetch_sub(1, Ordering::Relaxed);
                }
                vk if vk == VK_0 || vk == u32::from(VK_NUMPAD0) => {
                    STEPS.store(0, Ordering::Relaxed);
                    RESET.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    }
    // SAFETY: passes the event on unchanged.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}


fn down(key: VIRTUAL_KEY) -> bool {
    // SAFETY: reads the key's state; no pointers.
    (unsafe { GetAsyncKeyState(i32::from(key)) } as u16 & 0x8000) != 0
}


/// Whether a key litastum would press is held: its presses wait for them.
pub fn keys_held() -> bool {
    [VK_CONTROL, VK_MENU, VK_OEM_PLUS, VK_OEM_MINUS, VK_ADD, VK_SUBTRACT].into_iter().any(down)
}


/// Whether the window with the focus is Windows Terminal's: litastum's
/// presses go there, never into another program.
pub fn terminal_in_front() -> bool {
    let mut name = [0u16; 64];
    // SAFETY: the buffer and its length go together.
    let length = unsafe { GetClassNameW(GetForegroundWindow(), name.as_mut_ptr(), name.len() as i32) };
    String::from_utf16_lossy(&name[..length.max(0) as usize]) == TERMINAL_WINDOW_CLASS
}


/// Presses `Ctrl`+`+` (`presses` > 0) or `Ctrl`+`-` that many times, into
/// the window with the focus -- the terminal's own zoom keys.
pub fn press(presses: i32) {
    if presses == 0 {
        return;
    }
    let key = if presses > 0 { VK_OEM_PLUS } else { VK_OEM_MINUS };
    let event = |key: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: key, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { 0 }, time: 0, dwExtraInfo: 0 } },
    };
    let mut inputs = vec![event(VK_CONTROL, false)];
    for _ in 0..presses.unsigned_abs() {
        inputs.push(event(key, false));
        inputs.push(event(key, true));
    }
    inputs.push(event(VK_CONTROL, true));
    // SAFETY: `inputs` is a live array of `INPUT`s, its length and element
    // size passed alongside.
    unsafe {
        SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
    }
}
