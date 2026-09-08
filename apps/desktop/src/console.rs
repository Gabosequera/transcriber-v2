//! Consola integrada: captura eventos `tracing` en memoria (acotados) y los
//! escribe también a un archivo de log rotado por tamaño.

use crossbeam_channel::{Receiver, Sender, unbounded};
use std::io::Write;
use std::sync::Mutex;
use tracing::Level;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Clone, Debug)]
pub struct ConsoleLine {
    pub level: Level,
    pub target: String,
    pub message: String,
}

pub type ConsoleSink = Sender<ConsoleLine>;

struct ChannelLayer {
    tx: ConsoleSink,
    file: Mutex<Option<std::fs::File>>,
}

struct MessageVisitor(String);

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(&format!("{}={value:?}", field.name()));
        }
    }
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.0 = value.to_string();
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            self.0.push_str(&format!("{}={value}", field.name()));
        }
    }
}

impl<S: tracing::Subscriber> Layer<S> for ChannelLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut v = MessageVisitor(String::new());
        event.record(&mut v);
        let line = ConsoleLine { level: *event.metadata().level(), target: event.metadata().target().to_string(), message: v.0 };
        if let Ok(mut f) = self.file.lock()
            && let Some(f) = f.as_mut()
        {
            let _ = writeln!(f, "{} [{}] {}: {}", tv2_domain::project::now_iso(), line.level, line.target, redact(&line.message));
        }
        let _ = self.tx.send(line);
    }
}

/// Oculta valores que parezcan secretos (`api_key=…`, `token …`).
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for word in text.split(' ') {
        let lower = word.to_ascii_lowercase();
        if (lower.contains("key=") || lower.contains("token=") || lower.contains("secret=") || lower.contains("password=")) && word.len() > 8 {
            let idx = word.find('=').unwrap_or(0);
            out.push_str(&word[..=idx]);
            out.push_str("[redactado]");
        } else {
            out.push_str(word);
        }
        out.push(' ');
    }
    out.trim_end().to_string()
}

pub fn install_tracing() -> (ConsoleSink, Receiver<ConsoleLine>) {
    let (tx, rx) = unbounded();
    let file = open_log_file();
    let layer = ChannelLayer { tx: tx.clone(), file: Mutex::new(file) };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,wgpu=warn,naga=warn,egui=warn,eframe=warn,winit=warn,cpal=warn"));
    let registry = tracing_subscriber::registry().with(filter).with(layer);
    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_target(false).compact());
    let _ = registry.try_init();
    (tx, rx)
}

fn open_log_file() -> Option<std::fs::File> {
    let dir = crate::paths::logs_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("transcriptor.log");
    // rotación simple por tamaño (5 MB, un respaldo)
    if let Ok(meta) = std::fs::metadata(&path)
        && meta.len() > 5 * 1024 * 1024
    {
        let _ = std::fs::rename(&path, dir.join("transcriptor.1.log"));
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
}
