//! Keep the widget above the desktop shell and below ordinary application windows.
//! Do not hide it when applications open or move its z-order while the user is typing.
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use tauri::Manager;

#[derive(Default)]
pub struct Desktop {
    hidden: AtomicBool,
    pending: AtomicBool,
    hook_thread: AtomicU32,
}

pub fn set_hidden(app: &tauri::AppHandle, hidden: bool) {
    app.state::<Desktop>()
        .hidden
        .store(hidden, Ordering::Relaxed);
    queue_refresh(app);
}

fn queue_refresh(app: &tauri::AppHandle) {
    if app.state::<Desktop>().pending.swap(true, Ordering::Relaxed) {
        return;
    }
    let handle = app.clone();
    if app
        .run_on_main_thread(move || {
            handle
                .state::<Desktop>()
                .pending
                .store(false, Ordering::Relaxed);
            refresh(&handle);
        })
        .is_err()
    {
        app.state::<Desktop>()
            .pending
            .store(false, Ordering::Relaxed);
    }
}

#[cfg(windows)]
mod native {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        Graphics::Dwm::*,
        UI::WindowsAndMessaging::*,
    };

    pub unsafe fn class(hwnd: HWND) -> String {
        let mut name = [0u16; 128];
        let length = unsafe { GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32) };
        String::from_utf16_lossy(&name[..length.max(0) as usize])
    }

    fn shell(class: &str) -> bool {
        matches!(
            class,
            "Progman"
                | "WorkerW"
                | "Shell_TrayWnd"
                | "Shell_SecondaryTrayWnd"
                | "ApplicationManager_DesktopShellWindow"
        )
    }

    struct Scan {
        widget: HWND,
        seen_widget: bool,
        app_below: bool,
        lowest_application: HWND,
        foreground_desktop: HWND,
        desktop_seen: bool,
    }
    unsafe extern "system" fn candidate(hwnd: HWND, data: LPARAM) -> i32 {
        let scan = unsafe { &mut *(data as *mut Scan) };
        if hwnd == scan.foreground_desktop {
            scan.desktop_seen = true;
        }
        if hwnd == scan.widget {
            scan.seen_widget = true;
            return 1;
        }
        // Show Desktop can leave restored application HWNDs below the active desktop band.
        // They are not actually on the exposed desktop yet and must not sink the widget below it.
        if scan.desktop_seen && !scan.foreground_desktop.is_null() {
            return 1;
        }
        unsafe {
            if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
                return 1;
            }
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let extended = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if style & WS_CHILD != 0 || extended & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) != 0 {
                return 1;
            }
            // Other virtual desktops and shell-cloaked windows must not hide this desktop's widget.
            let mut cloaked = 0u32;
            let result = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED as u32,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            if result >= 0 && cloaked != 0 {
                return 1;
            }
            if shell(&class(hwnd)) {
                return 1;
            }
            // Ignore untitled infrastructure windows, while retaining borderless application windows.
            if style & WS_CAPTION == 0 && GetWindowTextLengthW(hwnd) == 0 {
                return 1;
            }
        }
        scan.lowest_application = hwnd;
        scan.app_below |= scan.seen_widget;
        1
    }

    pub fn place_on_desktop(widget: HWND) {
        let foreground = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) };
        let desktop = matches!(
            unsafe { class(foreground) }.as_str(),
            "Progman" | "WorkerW" | "ApplicationManager_DesktopShellWindow"
        );
        let mut scan = Scan {
            widget,
            seen_widget: false,
            app_below: false,
            lowest_application: std::ptr::null_mut(),
            foreground_desktop: if desktop {
                foreground
            } else {
                std::ptr::null_mut()
            },
            desktop_seen: false,
        };
        unsafe {
            if EnumWindows(Some(candidate), (&mut scan as *mut Scan) as LPARAM) == 0 {
                return;
            }
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER;
            if !scan.lowest_application.is_null() {
                // Only lower when needed: leave an active IME/editor completely alone otherwise.
                if scan.app_below
                    || GetWindowLongPtrW(widget, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0
                {
                    SetWindowPos(widget, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
                    SetWindowPos(widget, scan.lowest_application, 0, 0, 0, 0, flags);
                }
                return;
            }
            if GetAncestor(GetForegroundWindow(), GA_ROOT) == widget {
                return;
            }
            // Win+D can put the shell in front without hiding or minimizing this window.
            // Repair only that case, never repeatedly pulse the z-order of an idle editor.
            let mut rect = std::mem::zeroed();
            if GetWindowRect(widget, &mut rect) == 0 {
                return;
            }
            let point = windows_sys::Win32::Foundation::POINT {
                x: rect.left + 30,
                y: rect.top + 30,
            };
            let covering = GetAncestor(WindowFromPoint(point), GA_ROOT);
            if shell(&class(covering)) {
                SetWindowPos(widget, HWND_TOPMOST, 0, 0, 0, 0, flags);
                SetWindowPos(widget, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
            }
        }
    }
}

#[cfg(windows)]
fn refresh(app: &tauri::AppHandle) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else { return };
    unsafe {
        if app.state::<Desktop>().hidden.load(Ordering::Relaxed) {
            if IsWindowVisible(hwnd.0) != 0 {
                ShowWindow(hwnd.0, SW_HIDE);
            }
        } else {
            if IsWindowVisible(hwnd.0) == 0 || IsIconic(hwnd.0) != 0 {
                ShowWindow(hwnd.0, SW_SHOWNOACTIVATE);
            }
            native::place_on_desktop(hwnd.0);
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
    use std::sync::atomic::Ordering;
    use tauri::Manager;
    use windows_sys::Win32::{
        System::Threading::GetCurrentThreadId,
        UI::{Accessibility::*, WindowsAndMessaging::*},
    };
    thread_local! {
        static APP: RefCell<Option<tauri::AppHandle>> = const { RefCell::new(None) };
    }
    fn schedule() {
        APP.with(|slot| {
            if let Some(app) = slot.borrow().as_ref() {
                super::queue_refresh(app);
            }
        });
    }
    unsafe extern "system" fn changed(
        _: HWINEVENTHOOK,
        event: u32,
        hwnd: windows_sys::Win32::Foundation::HWND,
        object: i32,
        child: i32,
        _: u32,
        _: u32,
    ) {
        // Child controls generate many events; only top-level window changes matter.
        if event >= EVENT_OBJECT_CREATE {
            if object != OBJID_WINDOW || child != 0 {
                return;
            }
            if !hwnd.is_null()
                && unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd
                && event != EVENT_OBJECT_DESTROY
            {
                return;
            }
        }
        schedule();
    }
    pub fn start(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
        let handle = app.clone();
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("desktop-events".into())
            .spawn(move || {
                APP.with(|slot| *slot.borrow_mut() = Some(handle));
                unsafe {
                    let ranges = [
                        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
                        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
                        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
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
                    let timer = SetTimer(std::ptr::null_mut(), 0, 750, None);
                    if timer == 0 {
                        let error = std::io::Error::last_os_error();
                        for hook in hooks {
                            UnhookWinEvent(hook);
                        }
                        let _ = send.send(Err(error));
                        return;
                    }
                    APP.with(|slot| {
                        slot.borrow()
                            .as_ref()
                            .unwrap()
                            .state::<super::Desktop>()
                            .hook_thread
                            .store(GetCurrentThreadId(), Ordering::Relaxed)
                    });
                    let _ = send.send(Ok(()));
                    let mut message = std::mem::zeroed();
                    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                        if message.message == WM_TIMER && message.wParam == timer {
                            schedule();
                        } else {
                            TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    KillTimer(std::ptr::null_mut(), timer);
                    for hook in hooks {
                        UnhookWinEvent(hook);
                    }
                    APP.with(|slot| *slot.borrow_mut() = None);
                }
            })?;
        receive.recv()??;
        super::queue_refresh(app);
        Ok(())
    }
}

pub fn start(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(windows)]
    hook::start(app)?;
    #[cfg(not(windows))]
    queue_refresh(app);
    Ok(())
}

pub fn stop(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        let thread = app
            .state::<Desktop>()
            .hook_thread
            .swap(0, Ordering::Relaxed);
        if thread != 0 {
            unsafe {
                PostThreadMessageW(thread, WM_QUIT, 0, 0);
            }
        }
    }
}
