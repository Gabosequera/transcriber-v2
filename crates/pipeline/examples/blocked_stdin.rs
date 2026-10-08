//! Supervisor regression with owned Python transport fixtures, no ASR/inference.
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tv2_application::{ProjectSession, jobs};
use tv2_domain::{ErrorCode, Project};
use tv2_pipeline::{Parameters, Runtime};

#[cfg(windows)]
struct ProcessHandle(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
unsafe impl Send for ProcessHandle {}
#[cfg(windows)]
impl ProcessHandle {
    fn open(pid: u32) -> std::io::Result<Self> {
        use windows_sys::Win32::System::Threading::*;
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        Ok(Self(handle))
    }
    fn exited(&self) -> bool {
        unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 1000) == windows_sys::Win32::Foundation::WAIT_OBJECT_0 }
    }
}
#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            // Handles were opened for processes created by this fresh fixture.
            // Cleanup never selects a process by a potentially reused PID.
            if windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 0) != windows_sys::Win32::Foundation::WAIT_OBJECT_0 {
                let _ = windows_sys::Win32::System::Threading::TerminateProcess(self.0, 1);
            }
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn exercise(runtime: Runtime, source: PathBuf) -> Result<Value, Box<dyn std::error::Error>> {
    let tools = tv2_media::FfmpegTools::locate()?;
    let session = ProjectSession::new(Project::new("Owned blocked stdin regression"));
    let imported = tools.import(&source, source.to_string_lossy().into_owned())?;
    let mut asset = imported.clone();
    asset.extra.insert("transport_fixture_padding".into(), json!("x".repeat(128 * 1024)));
    let cancel = Arc::new(AtomicBool::new(false));
    let record = tv2_pipeline::enqueue(&session, asset, source.clone(), runtime.clone(), Parameters::default(), &tools, &cancel)?;
    let request_bytes = serde_json::to_vec(&json!({"protocol":tv2_pipeline::PROTOCOL,"id":"run","method":"run","params":{
        "job_id":record.id,"project_id":record.project_id,"revision":record.revision,"project_digest":record.payload.project_digest,
        "asset":record.payload.asset,"source_path":record.payload.source_path,"source_sha256":record.payload.source_sha256,
        "model_path":record.payload.runtime.model,"model_digest":record.payload.model_digest,"parameters":record.payload.parameters,"ffmpeg_path":record.payload.ffmpeg_path
    }}))?.len();
    if !(64 * 1024..1024 * 1024).contains(&request_bytes) {
        return Err("Blocked write fixture must exceed 64 KiB and fit the NDJSON limit".into());
    }
    let ready_path = runtime.work_root.join("blocked-stdin-ready.json");
    let cancellation = cancel.clone();
    let monitor = std::thread::spawn(move || -> Result<_, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if ready_path.is_file() {
                let ready: Value = serde_json::from_slice(&fs::read(&ready_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
                let pid = |key: &str| ready[key].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| "Invalid owned PID".to_owned());
                let handles = (
                    ProcessHandle::open(pid("worker_pid")?).map_err(|e| e.to_string())?,
                    ProcessHandle::open(pid("child_pid")?).map_err(|e| e.to_string())?,
                );
                std::thread::sleep(Duration::from_millis(300));
                cancellation.store(true, Ordering::Release);
                return Ok((handles, ready));
            }
            if Instant::now() >= deadline {
                cancellation.store(true, Ordering::Release);
                return Err("Fixture did not acknowledge hello".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let started = Instant::now();
    let result = tv2_pipeline::run(&record, false, cancel, |_| {});
    let elapsed = started.elapsed();
    let ((worker, child), ready) = monitor.join().map_err(|_| "Owned-process monitor panicked")??;
    let error = result.err().ok_or("Blocked stdin must never produce an analysis result")?;
    if error.code != ErrorCode::Cancelled || elapsed >= Duration::from_secs(5) {
        return Err(format!("Expected bounded Cancelled, observed {:?} after {elapsed:?}", error.code).into());
    }
    let worker_exited = worker.exited();
    let child_exited = child.exited();
    if !worker_exited || !child_exited {
        return Err("Owned worker or child survived cancellation".into());
    }
    let persisted = jobs::discover::<tv2_pipeline::AnalysisPayload>(&runtime.work_root.join("jobs"))?
        .into_iter()
        .find(|job| job.id == record.id)
        .ok_or("Job disappeared")?;
    if persisted.state != jobs::JobState::Cancelled {
        return Err("Cancellation was not durable".into());
    }

    let overflow_runtime = Runtime { work_root: runtime.work_root.join("overlimit"), ..runtime };
    let mut oversized = imported;
    oversized.extra.insert("transport_fixture_padding".into(), json!("x".repeat(1024 * 1024)));
    let overflow_cancel = Arc::new(AtomicBool::new(false));
    let overflow = tv2_pipeline::enqueue(&session, oversized, source, overflow_runtime.clone(), Parameters::default(), &tools, &overflow_cancel)?;
    let overflow_started = Instant::now();
    let overflow_error = tv2_pipeline::run(&overflow, false, overflow_cancel, |_| {}).err().ok_or("Oversized request unexpectedly succeeded")?;
    let overflow_elapsed = overflow_started.elapsed();
    if overflow_error.code != ErrorCode::Invalid || overflow_elapsed >= Duration::from_secs(5) {
        return Err("Oversized request was not rejected promptly as Invalid".into());
    }
    let overflow_job = jobs::discover::<tv2_pipeline::AnalysisPayload>(&overflow_runtime.work_root.join("jobs"))?
        .into_iter()
        .find(|job| job.id == overflow.id)
        .ok_or("Overflow job disappeared")?;
    if overflow_job.state != jobs::JobState::Failed {
        return Err("Oversized request failure was not durable".into());
    }
    Ok(json!({"transport_fixture_only":true,"inference_accepted":false,"accepted":true,
        "blocked_write":{"padding_bytes":128*1024,"request_bytes":request_bytes,"cancel_after_hello_ms":300,"elapsed_seconds":elapsed.as_secs_f64(),"error":error,"owned_handles":ready,"worker_handle_signaled":worker_exited,"child_handle_signaled":child_exited,"durable_state":persisted.state,"job_id":record.id},
        "oversized_request":{"padding_bytes":1024*1024,"elapsed_seconds":overflow_elapsed.as_secs_f64(),"error":overflow_error,"durable_state":overflow_job.state,"job_id":overflow.id}}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if arguments.len() != 6 {
        return Err("blocked_stdin PYTHON FIXTURE_WORKER MODEL NEW_WORK_ROOT SOURCE RESULT_JSON".into());
    }
    let runtime =
        Runtime { python: arguments[0].clone(), worker: arguments[1].clone(), model: arguments[2].clone(), work_root: arguments[3].clone() };
    if runtime.work_root.exists() {
        return Err("Use a new owned work root".into());
    }
    #[cfg(windows)]
    let result = exercise(runtime, arguments[4].clone())?;
    #[cfg(not(windows))]
    let result = {
        let _ = runtime;
        return Err("This regression requires Windows Job Object process handles".into());
    };
    fs::write(&arguments[5], serde_json::to_vec_pretty(&result)?)?;
    println!("PASS: blocked stdin cancellation, owned worker/child shutdown and oversized request rejection; no inference.");
    Ok(())
}
