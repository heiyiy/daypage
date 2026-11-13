//! Show the widget only while the desktop (or the widget itself) has focus.
//! A foreground WinEvent hook avoids polling and never inspects task/journal data.
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

#[derive(Default)]
pub struct Desktop {
    hidden: AtomicBool,
}

pub fn set_hidden(app: &tauri::AppHandle, hidden: bool) {
    app.state::<Desktop>()
        .hidden
        .store(hidden, Ordering::Relaxed);
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || refresh(&handle));
}

#[cfg(windows)]
fn refresh(app: &tauri::AppHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else { return };
    // Called on the UI thread. Never activate the widget when restoring it.
    unsafe {
        let foreground = GetForegroundWindow();
        let root = GetAncestor(foreground, GA_ROOT);
        let mut class = [0u16; 128];
        let length = GetClassNameW(root, class.as_mut_ptr(), class.len() as i32);
        let class = String::from_utf16_lossy(&class[..length.max(0) as usize]);
        let desktop = class == "Progman" || class == "WorkerW";
        let wanted =
            !app.state::<Desktop>().hidden.load(Ordering::Relaxed) && (desktop || root == hwnd.0);
        if wanted {
            if IsWindowVisible(hwnd.0) == 0 || IsIconic(hwnd.0) != 0 {
                ShowWindow(hwnd.0, SW_SHOWNOACTIVATE);
            }
        } else if IsWindowVisible(hwnd.0) != 0 {
            ShowWindow(hwnd.0, SW_HIDE);
        }
    }
}

#[cfg(not(windows))]
fn refresh(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if app.state::<Desktop>().hidden.load(Ordering::Relaxed) {
            let _ = window.hide();
        } else {
            let _ = window.show();
        }
    }
}

#[cfg(windows)]
mod hook {
    use std::cell::RefCell;
    use tauri::Manager;
    use windows_sys::Win32::UI::{Accessibility::*, WindowsAndMessaging::*};
    thread_local! {
        static APP: RefCell<Option<(tauri::AppHandle, usize)>> = const { RefCell::new(None) };
    }

    unsafe extern "system" fn changed(
        _: HWINEVENTHOOK,
        event: u32,
        hwnd: windows_sys::Win32::Foundation::HWND,
        _: i32,
        _: i32,
        _: u32,
        _: u32,
    ) {
        APP.with(|slot| {
            if let Some((app, widget)) = slot.borrow().as_ref() {
                if event != EVENT_SYSTEM_FOREGROUND && hwnd as usize != *widget {
                    return;
                }
                let handle = app.clone();
                // Re-read foreground on the UI thread, ignoring stale queued events.
                let _ = app.run_on_main_thread(move || super::refresh(&handle));
            }
        });
    }

    pub fn start(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
        let widget = app
            .get_webview_window("main")
            .ok_or("找不到待办窗口")?
            .hwnd()?
            .0 as usize;
        let handle = app.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("desktop-events".into())
            .spawn(move || {
                APP.with(|slot| *slot.borrow_mut() = Some((handle, widget)));
                unsafe {
                    // The shell can minimize/hide the widget after changing focus to the desktop.
                    // Observe those events too so Show Desktop cannot leave it hidden indefinitely.
                    let ranges = [
                        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                        (EVENT_OBJECT_HIDE, EVENT_OBJECT_HIDE),
                    ];
                    let mut hooks = Vec::new();
                    for (first, last) in ranges {
                        let hook = SetWinEventHook(
                            first,
                            last,
                            std::ptr::null_mut(),
                            Some(changed),
                            0,
                            0,
                            WINEVENT_OUTOFCONTEXT,
                        );
                        if hook.is_null() {
                            let error = std::io::Error::last_os_error();
                            for hook in hooks {
                                UnhookWinEvent(hook);
                            }
                            let _ = send.send(Err(error));
                            return;
                        }
                        hooks.push(hook);
                    }
                    let _ = send.send(Ok(()));
                    let mut message = std::mem::zeroed();
                    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                        TranslateMessage(&message);
                        DispatchMessageW(&message);
                    }
                    for hook in hooks {
                        UnhookWinEvent(hook);
                    }
                }
            })?;
        receive.recv()??;
        super::refresh(app);
        Ok(())
    }
}

pub fn start(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    hook::start(app)?;
    #[cfg(not(windows))]
    refresh(app);
    Ok(())
}
