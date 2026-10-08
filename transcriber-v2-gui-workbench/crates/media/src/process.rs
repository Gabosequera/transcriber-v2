//! Propiedad de procesos: cancelación desbloquea también lecturas/escrituras de pipe.
use parking_lot::Mutex;
use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

pub(crate) struct CancellableChild {
    child: Arc<Mutex<Child>>,
    done: Arc<AtomicBool>,
    watcher: Option<JoinHandle<()>>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
    pub stdin: Option<ChildStdin>,
}

impl CancellableChild {
    pub fn spawn(command: &mut Command, cancel: Arc<AtomicBool>) -> io::Result<Self> {
        let mut child = command.spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = child.stdin.take();
        let child = Arc::new(Mutex::new(child));
        let done = Arc::new(AtomicBool::new(false));
        let (process, stopped) = (child.clone(), done.clone());
        let watcher = std::thread::Builder::new().name("process-cancel".into()).spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                if cancel.load(Ordering::Acquire) {
                    let _ = process.lock().kill();
                    break;
                }
                if process.lock().try_wait().ok().flatten().is_some() {
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(20));
            }
        });
        match watcher {
            Ok(watcher) => Ok(Self { child, done, watcher: Some(watcher), stdout, stderr, stdin }),
            Err(e) => {
                let mut child = child.lock();
                let _ = child.kill();
                let _ = child.wait();
                Err(e)
            }
        }
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.lock().kill()
    }
    pub fn output(mut self) -> io::Result<std::process::Output> {
        use std::io::Read;
        let stderr = self.stderr.take();
        std::thread::scope(|scope| {
            let reader = scope.spawn(move || {
                let mut bytes = Vec::new();
                if let Some(mut pipe) = stderr {
                    pipe.read_to_end(&mut bytes)?;
                }
                Ok::<_, io::Error>(bytes)
            });
            let mut stdout = Vec::new();
            if let Some(mut pipe) = self.stdout.take() {
                pipe.read_to_end(&mut stdout)?;
            }
            let status = self.wait()?;
            let stderr = reader.join().map_err(|_| io::Error::other("stderr reader panicked"))??;
            Ok(std::process::Output { status, stdout, stderr })
        })
    }
    pub fn wait(&mut self) -> io::Result<ExitStatus> {
        loop {
            if let Some(status) = self.child.lock().try_wait()? {
                return Ok(status);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for CancellableChild {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        let _ = self.kill();
        let _ = self.wait();
        if let Some(watcher) = self.watcher.take() {
            watcher.thread().unpark();
            let _ = watcher.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_unblocks_ffmpeg_waiting_for_input() {
        use std::io::Read;
        let tools = crate::FfmpegTools::locate().unwrap();
        let mut command = tools.ffmpeg_cmd();
        command.args(["-f", "s16le", "-ar", "48000", "-ac", "1", "-i", "pipe:0", "-f", "f32le", "pipe:1"]);
        command.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
        let cancel = Arc::new(AtomicBool::new(false));
        let mut process = CancellableChild::spawn(&mut command, cancel.clone()).unwrap();
        let mut stdout = process.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let _ = tx.send(stdout.read(&mut [0; 8]));
        });
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "FFmpeg espera datos");
        cancel.store(true, Ordering::Release);
        assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap().unwrap(), 0);
        assert!(!process.wait().unwrap().success());
        reader.join().unwrap();
    }
}
