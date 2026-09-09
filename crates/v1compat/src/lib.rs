//! Compatibilidad con los contratos JSON de V1 (revisión `c3677ba`).
//!
//! Límites del adaptador:
//! - Lee y escribe los documentos V1 sin alterar su semántica: IDs, rangos,
//!   estados, `edited`, tombstones, `revision` y campos desconocidos se conservan.
//! - Los tiempos V1 son segundos con 3 decimales; se convierten a `Ticks`
//!   (exacto en ms) y vuelven a segundos con el mismo redondeo.
//! - El montaje V1 se convierte con un **perfil explícito** (`montaje::flatten`):
//!   la pista superior sustituye video y audio y los huecos se compactan; el
//!   resultado observable es el de V1, no las reglas de composición V2.
//! - La identidad del medio es la de V1 (`size + hash_muestreado + inventario_sha256`).

pub mod export;
pub mod import;
pub mod layers;
pub mod master;
pub mod montaje;
pub mod trims;

use tv2_domain::time::Ticks;

/// Segundos V1 (float) → ticks, con el redondeo a milisegundos de V1.
pub fn secs_to_ticks(v: f64) -> Ticks {
    Ticks::from_millis((v * 1000.0).round() as i64)
}

/// Ticks → segundos V1 (3 decimales).
pub fn ticks_to_secs(t: Ticks) -> f64 {
    t.as_seconds_ms()
}

/// Número JSON con la forma V1: entero si es exacto, si no float con 3 decimales.
pub fn secs_json(t: Ticks) -> serde_json::Value {
    let s = ticks_to_secs(t);
    serde_json::Value::from(s)
}

#[derive(Debug, thiserror::Error)]
pub enum V1Error {
    #[error("{0}")]
    Invalid(String),
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type V1Result<T> = Result<T, V1Error>;

pub fn invalid(msg: impl Into<String>) -> V1Error {
    V1Error::Invalid(msg.into())
}
