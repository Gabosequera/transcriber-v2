//! Killing this owned job never targets another editor or unrelated process.
#[cfg(windows)]
pub struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    pub fn attach(child: &std::process::Child) -> std::io::Result<Self> {
        use std::{
            mem::{size_of, zeroed},
            os::windows::io::AsRawHandle,
        };
        use windows_sys::Win32::{Foundation::CloseHandle, System::JobObjects::*};
        // Production starts the process suspended, assigns this job, then
        // resumes it; a venv launcher cannot spawn outside the owned job.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle() as _) == 0
            {
                let error = std::io::Error::last_os_error();
                CloseHandle(job);
                return Err(error);
            }
            Ok(Self(job))
        }
    }
    pub fn resume_suspended(&self, child: &std::process::Child) -> std::io::Result<()> {
        use std::mem::{size_of, zeroed};
        use windows_sys::Win32::{
            Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
            System::{Diagnostics::ToolHelp::*, Threading::*},
        };
        // Child's retained process handle and suspended main thread keep its
        // identity alive. Only that process's primary thread may be resumed.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let result = (|| {
                let mut entry: THREADENTRY32 = zeroed();
                entry.dwSize = size_of::<THREADENTRY32>() as u32;
                let mut found = Thread32First(snapshot, &mut entry);
                let mut thread_id = None;
                while found != 0 {
                    if entry.th32OwnerProcessID == child.id() && thread_id.replace(entry.th32ThreadID).is_some() {
                        return Err(std::io::Error::other("Worker suspendido con varios hilos inesperados"));
                    }
                    found = Thread32Next(snapshot, &mut entry);
                }
                let thread_id = thread_id.ok_or_else(|| std::io::Error::other("Hilo principal del worker no encontrado"))?;
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, thread_id);
                if thread.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let previous = ResumeThread(thread);
                let error = (previous == u32::MAX).then(std::io::Error::last_os_error);
                CloseHandle(thread);
                if let Some(error) = error {
                    return Err(error);
                }
                if previous != 1 {
                    return Err(std::io::Error::other("Estado inesperado al iniciar el worker suspendido"));
                }
                Ok(())
            })();
            CloseHandle(snapshot);
            result
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[cfg(not(windows))]
pub struct ProcessTree;
#[cfg(not(windows))]
impl ProcessTree {
    pub fn attach(_: &std::process::Child) -> std::io::Result<Self> {
        Ok(Self)
    }
}
