//! Windows keyboard redirection for hosts that never deliver key messages to plugin child windows
//! (FL Studio keeps them for its own shortcuts). A `WH_GETMESSAGE` hook on the GUI thread retargets
//! keyboard messages to the plugin window while one of its text fields is active, i.e. after the
//! user clicked inside the plugin and until they click somewhere else.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use winapi::shared::minwindef::{LPARAM, LRESULT, WPARAM};
use winapi::shared::windef::HWND;
use winapi::um::processthreadsapi::GetCurrentThreadId;
use winapi::um::winuser::{
    CallNextHookEx, IsChild, SetWindowsHookExW, HC_ACTION, MSG, PM_REMOVE,
    WH_GETMESSAGE, WM_CHAR, WM_DEADCHAR, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
    WM_NCLBUTTONDOWN, WM_RBUTTONDOWN, WM_SYSCHAR, WM_SYSDEADCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

/// The plugin window that was clicked last (0 = none).
static ACTIVE: AtomicIsize = AtomicIsize::new(0);
/// Whether egui currently has a text field focused in that window.
static WANTS_KEYS: AtomicBool = AtomicBool::new(false);

thread_local! {
    static HOOKED: Cell<bool> = const { Cell::new(false) };
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION && wparam as u32 == PM_REMOVE {
        let msg = &mut *(lparam as *mut MSG);
        let active = ACTIVE.load(Ordering::Relaxed) as HWND;
        match msg.message {
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_NCLBUTTONDOWN => {
                // A click outside the plugin window ends the redirection.
                if !active.is_null() && msg.hwnd != active && IsChild(active, msg.hwnd) == 0 {
                    ACTIVE.store(0, Ordering::Relaxed);
                }
            }
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR | WM_DEADCHAR
            | WM_SYSDEADCHAR => {
                if !active.is_null() && WANTS_KEYS.load(Ordering::Relaxed) && msg.hwnd != active {
                    msg.hwnd = active;
                }
            }
            _ => {}
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Call on every mouse-button press inside the plugin window.
pub fn activate(hwnd: HWND) {
    ACTIVE.store(hwnd as isize, Ordering::Relaxed);
    HOOKED.with(|h| {
        if !h.get() {
            // SAFETY: installing a thread-local message hook for the current (GUI) thread.
            let handle = unsafe { SetWindowsHookExW(WH_GETMESSAGE, Some(hook), std::ptr::null_mut(), GetCurrentThreadId()) };
            if !handle.is_null() {
                h.set(true);
            }
        }
    });
}

/// Call once per frame with `egui::Context::wants_keyboard_input()`.
pub fn set_wants_keys(wants: bool) {
    WANTS_KEYS.store(wants, Ordering::Relaxed);
}

/// Call when the plugin window closes.
pub fn deactivate(hwnd: HWND) {
    let _ = ACTIVE.compare_exchange(hwnd as isize, 0, Ordering::Relaxed, Ordering::Relaxed);
}
