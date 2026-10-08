//! Real Python hello/EOF and owned Windows process-tree smoke.
//! The cancellation fixture is transport-only; it performs no inference.
#[path = "../src/process_tree.rs"]
mod process_tree;

use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
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
        unsafe {
            let handle = windows_sys::Win32::System::Threading::OpenProcess(windows_sys::Win32::System::Threading::PROCESS_SYNCHRONIZE, 0, pid);
            if handle.is_null() {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self(handle))
        }
    }
    fn exited(&self) -> bool {
        unsafe { windows_sys::Win32::System::Threading::WaitForSingleObject(self.0, 10000) == windows_sys::Win32::Foundation::WAIT_OBJECT_0 }
    }
}
#[cfg(windows)]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

fn eof_smoke(runtime: &Runtime) -> Result<Value, Box<dyn std::error::Error>> {
    let mut command = Command::new(&runtime.python);
    command
        .args(["-I", "-u"])
        .arg(&runtime.worker)
        .arg("--work-root")
        .arg(runtime.work_root.join("eof"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000 | 0x00000004);
    }
    let mut child = command.spawn()?;
    let tree = match process_tree::ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error.into());
        }
    };
    #[cfg(windows)]
    if let Err(error) = tree.resume_suspended(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let result = (|| {
        let pid = child.id();
        let mut input = child.stdin.take().unwrap();
        serde_json::to_writer(&mut input, &json!({"protocol":tv2_pipeline::PROTOCOL,"id":"hello","method":"hello","params":{}}))?;
        input.write_all(b"\n")?;
        input.flush()?;
        let output = child.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(output).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let response: Value = serde_json::from_str(&receiver.recv_timeout(Duration::from_secs(15))??)?;
        if response["protocol"] != tv2_pipeline::PROTOCOL || response["result"]["protocol"] != tv2_pipeline::PROTOCOL {
            return Err("Real worker hello failed during EOF smoke".into());
        }
        drop(input); // EOF is the real worker's documented cancellation boundary.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err("Real worker did not exit successfully on EOF".into());
                }
                return Ok(json!({"worker_pid":pid,"hello":response,"eof_exit_code":status.code()}));
            }
            if Instant::now() >= deadline {
                return Err("Real worker did not exit within EOF grace".into());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    })();
    let _ = child.kill();
    let _ = child.wait();
    drop(tree);
    result
}

#[cfg(windows)]
fn tree_smoke(runtime: &Runtime, source: PathBuf, fixture: PathBuf) -> Result<Value, Box<dyn std::error::Error>> {
    let tools = tv2_media::FfmpegTools::locate()?;
    let session = ProjectSession::new(Project::new("Synthetic supervisor smoke"));
    let asset = tools.import(&source, source.to_string_lossy().into_owned())?;
    let fixture_runtime = Runtime { worker: fixture, work_root: runtime.work_root.join("tree-fixture"), ..runtime.clone() };
    let cancel = Arc::new(AtomicBool::new(false));
    let record = tv2_pipeline::enqueue(&session, asset, source, fixture_runtime, Parameters::default(), &tools, &cancel)?;
    let captured = Arc::new(Mutex::new(None::<(ProcessHandle, ProcessHandle, Value)>));
    let capture = captured.clone();
    let cancellation = cancel.clone();
    let observed = tv2_pipeline::run(&record, false, cancel, move |event| {
        let worker_pid = event["worker_pid"].as_u64().and_then(|value| u32::try_from(value).ok());
        let ffmpeg_pid = event["ffmpeg_pid"].as_u64().and_then(|value| u32::try_from(value).ok());
        if let (Some(worker_pid), Some(ffmpeg_pid)) = (worker_pid, ffmpeg_pid)
            && let (Ok(worker), Ok(ffmpeg)) = (ProcessHandle::open(worker_pid), ProcessHandle::open(ffmpeg_pid))
        {
            *capture.lock().unwrap() = Some((worker, ffmpeg, event));
        }
        // Never leave the deliberately unresponsive owned fixture running,
        // including on failure to obtain diagnostic handles.
        cancellation.store(true, Ordering::Release);
    });
    let error = observed.err().ok_or("Fixture must cancel, never claim a model result")?;
    if error.code != ErrorCode::Cancelled {
        return Err(format!("Wrong cancellation error: {error}").into());
    }
    let (worker, ffmpeg, event) = captured.lock().unwrap().take().ok_or("No owned-process handles were captured")?;
    if !worker.exited() || !ffmpeg.exited() {
        return Err("Owned Python/FFmpeg survived kill-on-close".into());
    }
    let persisted = jobs::discover::<tv2_pipeline::AnalysisPayload>(&record.payload.runtime.work_root.join("jobs"))?
        .into_iter()
        .find(|value| value.id == record.id)
        .ok_or("Cancelled job not persisted")?;
    if persisted.state != jobs::JobState::Cancelled {
        return Err("Durable job did not record cancellation".into());
    }
    Ok(json!({"transport_fixture_only":true,"inference_exercised":false,"event":event,
        "error":error,"worker_handle_signaled":true,"ffmpeg_handle_signaled":true,"job":persisted}))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if arguments.len() != 7 {
        return Err("runtime_smoke PYTHON WORKER MODEL WORK_ROOT SOURCE FIXTURE RESULT_JSON".into());
    }
    let runtime =
        Runtime { python: arguments[0].clone(), worker: arguments[1].clone(), model: arguments[2].clone(), work_root: arguments[3].clone() };
    if runtime.work_root.exists() {
        return Err("Use a new owned smoke work root".into());
    }
    fs::create_dir_all(&runtime.work_root)?;
    let started = Instant::now();
    let hello = tv2_pipeline::capabilities(&runtime, &AtomicBool::new(false))?;
    if hello["protocol"] != tv2_pipeline::PROTOCOL || hello["capabilities"]["transcribe"]["runtime_state"] != "not_loaded" {
        return Err("Real worker capability response is incompatible".into());
    }
    let cancelled = tv2_pipeline::capabilities(&runtime, &AtomicBool::new(true)).err().ok_or("Pre-cancelled capabilities unexpectedly succeeded")?;
    if cancelled.code != ErrorCode::Cancelled {
        return Err("Pre-cancelled hello did not preserve cancellation code".into());
    }
    let eof = eof_smoke(&runtime)?;
    #[cfg(windows)]
    let tree = tree_smoke(&runtime, arguments[4].clone(), arguments[5].clone())?;
    #[cfg(not(windows))]
    let tree = json!({"skipped":"This smoke validates Windows JobObject"});
    let result = json!({"protocol":tv2_pipeline::PROTOCOL,"accepted_host_smoke":true,"inference_accepted":false,
        "elapsed_seconds":started.elapsed().as_secs_f64(),"hello":hello,"pre_cancel_error":cancelled,"eof":eof,"tree":tree});
    fs::write(&arguments[6], serde_json::to_vec_pretty(&result)?)?;
    println!("PASS: real Python hello/EOF, cancelled host and owned Windows tree; inference untested. Evidence {}", arguments[6].display());
    Ok(())
}
