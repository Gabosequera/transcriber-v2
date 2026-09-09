//! OS notifications are hints. Validation and the stable fallback rescan remain authoritative.
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
pub struct Watcher {
    pub root: PathBuf,
    stop: Arc<AtomicBool>,
    result: crossbeam_channel::Receiver<Result<(), String>>,
}
impl Watcher {
    pub fn start(root: PathBuf, wake: egui::Context) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let token = stop.clone();
        let path = root.clone();
        let (tx, result) = crossbeam_channel::bounded(1);
        std::thread::Builder::new().name("project-directory-watch".into()).spawn(move || {
            let report = |event| {
                let _ = tx.try_send(event);
                wake.request_repaint();
            };
            if let Err(error) = watch(path, token, &report) {
                report(Err(error.to_string()));
            }
        })?;
        Ok(Self { root, stop, result })
    }
    pub fn changed(&self) -> Option<Result<(), String>> {
        self.result.try_recv().ok()
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

#[cfg(windows)]
fn watch(path: PathBuf, stop: Arc<AtomicBool>, report: &dyn Fn(Result<(), String>)) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::{INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT},
        Storage::FileSystem::{
            FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE,
            FindCloseChangeNotification, FindFirstChangeNotificationW, FindNextChangeNotification,
        },
        System::Threading::WaitForSingleObject,
    };
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if name[..name.len() - 1].contains(&0) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "ruta con NUL"));
    }
    // The handle and its UTF-16 input are confined to this worker. Closing it
    // occurs after the final wait; no thread can race a use against destruction.
    unsafe {
        let handle = FindFirstChangeNotificationW(
            name.as_ptr(),
            1,
            FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_DIR_NAME | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_LAST_WRITE,
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err(std::io::Error::last_os_error());
        }
        let result = (|| {
            while !stop.load(Ordering::Acquire) {
                match WaitForSingleObject(handle, 250) {
                    WAIT_OBJECT_0 => {
                        report(Ok(()));
                        if FindNextChangeNotification(handle) == 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    WAIT_TIMEOUT => {}
                    _ => return Err(std::io::Error::last_os_error()),
                }
            }
            Ok(())
        })();
        FindCloseChangeNotification(handle);
        result
    }
}
#[cfg(not(windows))]
fn watch(_: PathBuf, _: Arc<AtomicBool>, _: &dyn Fn(Result<(), String>)) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Watcher nativo disponible en Windows; rescaneo periódico activo"))
}
