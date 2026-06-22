#[cfg(target_os = "windows")]
mod imp {
    use eframe::egui;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::thread;

    type Handle = *mut c_void;

    const ERROR_ALREADY_EXISTS: u32 = 183;
    const EVENT_MODIFY_STATE: u32 = 0x0002;
    const INFINITE: u32 = 0xFFFF_FFFF;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_FAILED: u32 = 0xFFFF_FFFF;

    const MUTEX_NAME: &str = "Local\\TextSwitch.SingleInstance";
    const SHOW_EVENT_NAME: &str = "Local\\TextSwitch.ShowEditor";

    unsafe extern "system" {
        fn CloseHandle(handle: Handle) -> i32;
        fn CreateEventW(
            attributes: *mut c_void,
            manual_reset: i32,
            initial_state: i32,
            name: *const u16,
        ) -> Handle;
        fn CreateMutexW(attributes: *mut c_void, initial_owner: i32, name: *const u16) -> Handle;
        fn GetLastError() -> u32;
        fn OpenEventW(desired_access: u32, inherit_handle: i32, name: *const u16) -> Handle;
        fn SetEvent(event: Handle) -> i32;
        fn WaitForSingleObject(handle: Handle, milliseconds: u32) -> u32;
    }

    pub struct InstanceGuard {
        handle: Handle,
    }

    impl Drop for InstanceGuard {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    CloseHandle(self.handle);
                }
            }
        }
    }

    fn wide(name: &str) -> Vec<u16> {
        OsStr::new(name).encode_wide().chain([0]).collect()
    }

    pub fn acquire() -> Option<InstanceGuard> {
        let name = wide(MUTEX_NAME);
        let handle = unsafe { CreateMutexW(ptr::null_mut(), 1, name.as_ptr()) };
        if handle.is_null() {
            return Some(InstanceGuard { handle });
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return None;
        }
        Some(InstanceGuard { handle })
    }

    pub fn notify_existing() {
        let name = wide(SHOW_EVENT_NAME);
        let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if event.is_null() {
            return;
        }
        unsafe {
            SetEvent(event);
            CloseHandle(event);
        }
    }

    pub fn listen_for_show_requests(ctx: &egui::Context) {
        let name = wide(SHOW_EVENT_NAME);
        let event = unsafe { CreateEventW(ptr::null_mut(), 0, 0, name.as_ptr()) };
        if event.is_null() {
            return;
        }

        let event = event as usize;
        let ctx = ctx.clone();
        thread::spawn(move || {
            let event = event as Handle;
            loop {
                match unsafe { WaitForSingleObject(event, INFINITE) } {
                    WAIT_OBJECT_0 => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                    WAIT_FAILED => {
                        unsafe {
                            CloseHandle(event);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use eframe::egui;

    pub struct InstanceGuard;

    pub fn acquire() -> Option<InstanceGuard> {
        Some(InstanceGuard)
    }

    pub fn notify_existing() {}

    pub fn listen_for_show_requests(_ctx: &egui::Context) {}
}

pub use imp::*;
