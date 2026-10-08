//! Registro único de acciones y atajos. IDs, etiquetas, grupos y acordes por
//! defecto conservan los de `keymap.py` (V1). Alimenta teclado, toolbar,
//! menú contextual y documentación de capabilities. Las diferencias con V1 se
//! registran en `implementation/v1-action-inventory.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const SCHEMA: &str = "keymap/1";
pub const GROUPS: [&str; 7] = ["Transporte", "Navegación", "Edición", "Herramientas", "Vista", "Montaje", "Marcas"];

#[derive(Clone, Debug)]
#[allow(dead_code)] // `contexts` alimenta el menú contextual por tipo de selección (E3)
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub defaults: Vec<&'static str>,
    /// Contextos en los que aplica (vacío = siempre).
    pub contexts: &'static [Context],
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum Context {
    Clip,
    Item,
    Any,
}

macro_rules! actions {
    ($( $id:literal, $label:literal, $group:literal, [$($chord:literal),*], [$($ctx:expr),*] ;)*) => {
        pub fn registry() -> Vec<Action> {
            vec![$( Action { id: $id, label: $label, group: $group, defaults: vec![$($chord),*], contexts: &[$($ctx),*] } ),*]
        }
    };
}

actions! {
    "sequence.marker_add", "Añadir marcador de secuencia", "Marcas", [], [];
    "sequence.markers", "Lista de marcadores", "Marcas", [], [];
    "sequence.insert_selection", "Insertar selección en playhead (cierra origen)", "Montaje", [], [Context::Clip];
    "transport.play_pause", "Play / pausa", "Transporte", ["space"], [];
    "transport.pause", "Pausa", "Transporte", ["K"], [];
    "transport.faster", "Más rápido (×1→×2→×3→×4→×8; pausado = play a ×1)", "Transporte", ["L"], [];
    "transport.slower", "Más lento (a ×1 pausa)", "Transporte", ["J"], [];
    "transport.rate_1", "Velocidad ×1", "Transporte", ["1"], [];
    "transport.rate_2", "Velocidad ×2", "Transporte", ["2"], [];
    "transport.rate_3", "Velocidad ×3", "Transporte", ["3"], [];
    "transport.rate_4", "Velocidad ×4", "Transporte", ["4"], [];
    "transport.skim", "Skim ×8 (solo fotogramas clave)", "Transporte", ["Shift+L"], [];
    "transport.play_from_item", "Play desde el item seleccionado (o el IN)", "Transporte", ["Shift+space"], [];
    "nav.frame_prev", "Fotograma anterior", "Navegación", ["comma"], [];
    "nav.frame_next", "Fotograma siguiente", "Navegación", ["period"], [];
    "nav.frame_prev_10", "10 fotogramas atrás", "Navegación", ["Shift+comma"], [];
    "nav.frame_next_10", "10 fotogramas adelante", "Navegación", ["Shift+period"], [];
    "nav.step_prev", "Paso atrás (0,5 s)", "Navegación", ["Left"], [];
    "nav.step_next", "Paso adelante (0,5 s)", "Navegación", ["Right"], [];
    "nav.step_prev_5", "Paso atrás (5 s)", "Navegación", ["Shift+Left"], [];
    "nav.step_next_5", "Paso adelante (5 s)", "Navegación", ["Shift+Right"], [];
    "nav.home", "Inicio del medio", "Navegación", ["Home"], [];
    "nav.end", "Fin del medio", "Navegación", ["End"], [];
    "nav.prev_edge", "Borde anterior", "Navegación", ["Up"], [];
    "nav.next_edge", "Borde siguiente", "Navegación", ["Down"], [];
    "nav.prev_silence", "Silencio anterior", "Navegación", ["Ctrl+Up"], [];
    "nav.next_silence", "Silencio siguiente", "Navegación", ["Ctrl+Down"], [];
    "nav.goto", "Ir a tiempo…", "Navegación", ["Ctrl+G"], [];
    "nav.sel_start", "Inicio de la selección", "Navegación", ["Shift+I"], [];
    "nav.sel_end", "Fin de la selección", "Navegación", ["Shift+O"], [];
    "edit.split", "Dividir en el playhead", "Edición", ["S"], [Context::Clip];
    "edit.trim_start", "Recortar inicio al playhead", "Edición", ["bracketleft"], [Context::Clip];
    "edit.trim_end", "Recortar fin al playhead", "Edición", ["bracketright"], [Context::Clip];
    "edit.nudge_prev", "Empujar 1 fotograma atrás", "Edición", ["Alt+Left"], [Context::Clip];
    "edit.nudge_next", "Empujar 1 fotograma adelante", "Edición", ["Alt+Right"], [Context::Clip];
    "edit.nudge_prev_10", "Empujar 10 fotogramas atrás", "Edición", ["Alt+Shift+Left"], [Context::Clip];
    "edit.nudge_next_10", "Empujar 10 fotogramas adelante", "Edición", ["Alt+Shift+Right"], [Context::Clip];
    "edit.item_prev", "Item anterior del carril", "Edición", ["Shift+Tab"], [];
    "edit.item_next", "Item siguiente del carril", "Edición", ["Tab"], [];
    "edit.accept", "Aceptar (marca de revisión: se corta igual que un propuesto)", "Edición", ["E"], [Context::Item];
    "edit.accept_next", "Aceptar y pasar al siguiente", "Edición", ["Shift+E"], [Context::Item];
    "edit.toggle", "Desactivar (no se corta; en una marca cicla la decisión)", "Edición", ["X"], [Context::Item, Context::Clip];
    "edit.activate", "Activar: vuelve a propuesto (se corta)", "Edición", ["P"], [Context::Item, Context::Clip];
    "edit.delete", "Borrar", "Edición", ["Delete", "BackSpace", "D"], [Context::Item, Context::Clip];
    "edit.delete_ripple", "Eliminar con ripple (cierra el hueco)", "Edición", ["Shift+Delete"], [Context::Clip];
    "edit.edit", "Editar", "Edición", ["Return", "F2"], [Context::Item, Context::Clip];
    "edit.deselect", "Deseleccionar", "Edición", ["Escape"], [];
    "edit.undo", "Deshacer", "Edición", ["Ctrl+Z"], [];
    "edit.redo", "Rehacer", "Edición", ["Ctrl+R", "Ctrl+Shift+Z", "Ctrl+Y"], [];
    "edit.copy", "Copiar", "Edición", ["Ctrl+C"], [Context::Clip, Context::Item];
    "edit.cut", "Cortar", "Edición", ["Ctrl+X"], [Context::Clip, Context::Item];
    "edit.paste", "Pegar en el playhead", "Edición", ["Ctrl+V"], [];
    "edit.duplicate", "Duplicar", "Edición", ["Ctrl+D"], [Context::Clip, Context::Item];
    "tools.select", "Herramienta Selección", "Herramientas", ["A", "V"], [];
    "tools.cut", "Herramienta Corte", "Herramientas", ["B"], [];
    "tools.select_all", "Seleccionar todo el carril", "Herramientas", ["Ctrl+A"], [];
    "layers.new_lane", "Añadir capa…", "Herramientas", ["Ctrl+N"], [];
    "layers.add_range", "Añadir tramo IN/OUT a la capa seleccionada", "Herramientas", ["Ctrl+Return"], [];
    "view.zoom_in", "Zoom +", "Vista", ["plus", "equal", "KP_Add"], [];
    "view.zoom_out", "Zoom −", "Vista", ["minus", "KP_Subtract"], [];
    "view.fit", "Ver todo", "Vista", ["Shift+Z"], [];
    "view.zoom_sel", "Zoom a la selección", "Vista", ["Z"], [];
    "view.follow", "Seguir al playhead", "Vista", ["F"], [];
    "view.center", "Centrar el playhead", "Vista", ["C"], [];
    "view.skip_trims", "Saltar recortes al reproducir", "Vista", ["Shift+T"], [];
    "loop.set_in", "Repetir: entrada en el playhead", "Vista", [], [];
    "loop.set_out", "Repetir: salida en el playhead", "Vista", [], [];
    "loop.clear", "Repetir: quitar el rango", "Vista", [], [];
    "view.mode_montage", "Modo Fuente / Secuencia", "Montaje", ["Ctrl+M"], [];
    "montage.add_selection", "Añadir a la secuencia el rango seleccionado (IN/OUT o item)", "Montaje", ["Ctrl+Shift+A"], [];
    "montage.add_topic", "Añadir a la secuencia el tema seleccionado (todos sus tramos)", "Montaje", ["Ctrl+Shift+T"], [];
    "montage.reveal_source", "Ver el clip en la fuente", "Montaje", ["Ctrl+Shift+R"], [Context::Clip];
    "montage.move_up", "Subir el clip a la pista de encima", "Montaje", ["Ctrl+Shift+Up"], [Context::Clip];
    "montage.move_down", "Bajar el clip a la pista de abajo", "Montaje", ["Ctrl+Shift+Down"], [Context::Clip];
    "montage.export", "Exportar…", "Montaje", ["Ctrl+E"], [];
    "marks.point", "Marca puntual", "Marcas", ["M"], [];
    "marks.in", "IN de región", "Marcas", ["I"], [];
    "marks.out", "OUT de región", "Marcas", ["O"], [];
    "file.open", "Abrir proyecto…", "Herramientas", ["Ctrl+O"], [];
    "file.save", "Guardar proyecto", "Herramientas", ["Ctrl+S"], [];
    "file.import", "Importar medios…", "Herramientas", ["Ctrl+I"], [];
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct KeymapFile {
    pub schema: String,
    #[serde(default)]
    pub bindings: HashMap<String, Vec<String>>,
}

#[allow(dead_code)] // `overrides` y `save_overrides` se usan en Ajustes → Atajos (E3)
pub struct Keymap {
    pub actions: Vec<Action>,
    chords: HashMap<&'static str, Vec<String>>,
    bindings: HashMap<String, &'static str>,
    pub conflicts: Vec<(String, String, String)>,
    pub warnings: Vec<String>,
    pub overrides: HashMap<String, Vec<String>>,
}

impl Keymap {
    pub fn load() -> Keymap {
        let mut overrides = HashMap::new();
        let mut warnings = Vec::new();
        let path = crate::paths::keymap_file();
        if path.is_file() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| serde_json::from_str::<KeymapFile>(&t).map_err(|e| e.to_string()))
            {
                Ok(f) if f.schema == SCHEMA => overrides = f.bindings,
                Ok(f) => warnings.push(format!("keymap.json: schema desconocido {}; se usan los atajos por defecto", f.schema)),
                Err(e) => warnings.push(format!("keymap.json: {e}; se usan los atajos por defecto")),
            }
        }
        let mut km = Keymap::with_overrides(overrides);
        km.warnings.extend(warnings);
        km
    }

    pub fn with_overrides(overrides: HashMap<String, Vec<String>>) -> Keymap {
        let actions = registry();
        let mut chords: HashMap<&'static str, Vec<String>> = HashMap::new();
        let mut bindings: HashMap<String, &'static str> = HashMap::new();
        let mut conflicts = Vec::new();
        let mut warnings = Vec::new();
        let mut clean_overrides: HashMap<String, Vec<String>> = HashMap::new();
        for (id, list) in &overrides {
            if !actions.iter().any(|a| a.id == id) {
                warnings.push(format!("acción desconocida en keymap.json: {id}"));
                continue;
            }
            let mut clean = Vec::new();
            for c in list {
                match normalize_chord(c) {
                    Some(n) => clean.push(n),
                    None => warnings.push(format!("{id}: acorde inválido «{c}»")),
                }
            }
            clean_overrides.insert(id.clone(), clean);
        }
        for a in &actions {
            let list: Vec<String> = match clean_overrides.get(a.id) {
                Some(v) => v.clone(),
                None => a.defaults.iter().filter_map(|c| normalize_chord(c)).collect(),
            };
            for c in &list {
                if let Some(owner) = bindings.get(c) {
                    conflicts.push((c.clone(), owner.to_string(), a.id.to_string()));
                    continue;
                }
                bindings.insert(c.clone(), a.id);
            }
            chords.insert(a.id, list);
        }
        Keymap { actions, chords, bindings, conflicts, warnings, overrides: clean_overrides }
    }

    pub fn resolve(&self, chord: &str) -> Option<&'static str> {
        self.bindings.get(chord).copied()
    }

    pub fn chords(&self, id: &str) -> &[String] {
        self.chords.get(id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn pretty(&self, id: &str) -> String {
        self.chords(id).iter().map(|c| pretty_chord(c)).collect::<Vec<_>>().join(" · ")
    }

    pub fn tooltip(&self, id: &str) -> String {
        let label = self.actions.iter().find(|a| a.id == id).map(|a| a.label).unwrap_or(id);
        let p = self.pretty(id);
        if p.is_empty() { label.to_string() } else { format!("{label}  ·  {p}") }
    }

    pub fn label(&self, id: &str) -> &'static str {
        self.actions.iter().find(|a| a.id == id).map(|a| a.label).unwrap_or("")
    }
}

const MODS: [&str; 3] = ["Ctrl", "Alt", "Shift"];

fn key_alias(key: &str) -> String {
    let low = key.to_ascii_lowercase();
    let mapped = match low.as_str() {
        " " | "spacebar" | "space" => "space",
        "esc" | "escape" => "Escape",
        "enter" | "return" => "Return",
        "del" | "supr" | "delete" => "Delete",
        "backspace" => "BackSpace",
        "tab" => "Tab",
        "+" | "plus" => "plus",
        "-" | "minus" => "minus",
        "=" | "equal" => "equal",
        "," | "comma" => "comma",
        "." | "period" => "period",
        "[" | "bracketleft" => "bracketleft",
        "]" | "bracketright" => "bracketright",
        "up" | "arrowup" => "Up",
        "down" | "arrowdown" => "Down",
        "left" | "arrowleft" => "Left",
        "right" | "arrowright" => "Right",
        "home" => "Home",
        "end" => "End",
        "pageup" | "prior" => "Prior",
        "pagedown" | "next" => "Next",
        "insert" => "Insert",
        "kp_add" => "KP_Add",
        "kp_subtract" => "KP_Subtract",
        "semicolon" | ";" => "semicolon",
        "slash" | "/" => "slash",
        "backslash" | "\\" => "backslash",
        "apostrophe" | "'" => "apostrophe",
        "grave" | "`" => "grave",
        _ => "",
    };
    if !mapped.is_empty() {
        return mapped.to_string();
    }
    if key.chars().count() == 1 {
        return key.to_uppercase();
    }
    if low.starts_with('f') && low[1..].chars().all(|c| c.is_ascii_digit()) && low.len() <= 3 {
        return low.to_uppercase();
    }
    key.to_string()
}

pub fn normalize_chord(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text == "+" {
        return Some("plus".into());
    }
    let parts: Vec<&str> = if let Some(stripped) = text.strip_suffix("++") {
        let mut v: Vec<&str> = stripped.split('+').collect();
        v.push("+");
        v
    } else {
        text.split('+').collect()
    };
    let mut mods = Vec::new();
    let mut key: Option<String> = None;
    for (i, p) in parts.iter().enumerate() {
        let p = p.trim();
        if p.is_empty() {
            return None;
        }
        let m = match p.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => Some("Ctrl"),
            "alt" | "option" => Some("Alt"),
            "shift" => Some("Shift"),
            _ => None,
        };
        if let Some(m) = m
            && i < parts.len() - 1
        {
            if !mods.contains(&m) {
                mods.push(m);
            }
        } else if m.is_some() {
            return None;
        } else if key.is_none() {
            key = Some(key_alias(p));
        } else {
            return None;
        }
    }
    let key = key?;
    let ordered: Vec<&str> = MODS.iter().copied().filter(|m| mods.contains(m)).collect();
    let mut out = ordered.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&key);
    Some(out)
}

pub fn pretty_chord(chord: &str) -> String {
    chord
        .split('+')
        .map(|p| match p {
            "period" => ".",
            "comma" => ",",
            "bracketleft" => "[",
            "bracketright" => "]",
            "plus" => "+",
            "minus" => "-",
            "equal" => "=",
            "space" => "Espacio",
            "Delete" => "Supr",
            "BackSpace" => "Retroceso",
            "Return" => "Enter",
            "Escape" => "Esc",
            "Left" => "←",
            "Right" => "→",
            "Up" => "↑",
            "Down" => "↓",
            "Home" => "Inicio",
            "End" => "Fin",
            "KP_Add" => "Num +",
            "KP_Subtract" => "Num −",
            "Prior" => "RePág",
            "Next" => "AvPág",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Acorde a partir de un evento de tecla egui. Devuelve `None` para modificadores solos.
pub fn chord_from_egui(key: egui::Key, modifiers: egui::Modifiers) -> Option<String> {
    use egui::Key as K;
    let name = match key {
        K::Space => "space",
        K::Escape => "Escape",
        K::Enter => "Return",
        K::Delete => "Delete",
        K::Backspace => "BackSpace",
        K::Tab => "Tab",
        K::Plus => "plus",
        K::Minus => "minus",
        K::Equals => "equal",
        K::Comma => "comma",
        K::Period => "period",
        K::OpenBracket => "bracketleft",
        K::CloseBracket => "bracketright",
        K::ArrowUp => "Up",
        K::ArrowDown => "Down",
        K::ArrowLeft => "Left",
        K::ArrowRight => "Right",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "Prior",
        K::PageDown => "Next",
        K::Insert => "Insert",
        K::Semicolon => "semicolon",
        K::Slash => "slash",
        K::Backslash => "backslash",
        K::Quote => "apostrophe",
        K::Backtick => "grave",
        K::Num0 => "0",
        K::Num1 => "1",
        K::Num2 => "2",
        K::Num3 => "3",
        K::Num4 => "4",
        K::Num5 => "5",
        K::Num6 => "6",
        K::Num7 => "7",
        K::Num8 => "8",
        K::Num9 => "9",
        other => {
            let n = other.name();
            if n.len() == 1 || n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit()) {
                return build(n, modifiers);
            }
            return build(n, modifiers);
        }
    };
    build(name, modifiers)
}

fn build(key: &str, modifiers: egui::Modifiers) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    // En Windows AltGr llega como Ctrl+Alt: si la tecla no es letra/dígito ni
    // navegación, se trata como tecla sin modificadores (escritura de `[`, `]`, `@`).
    let mut ctrl = modifiers.ctrl || modifiers.command;
    let mut alt = modifiers.alt;
    let nav = ["Left", "Right", "Up", "Down", "Home", "End", "Prior", "Next", "Insert", "Delete", "BackSpace", "Tab", "Return", "Escape", "space"];
    if cfg!(windows) && ctrl && alt {
        let alnum = key.len() == 1 && key.chars().all(|c| c.is_ascii_alphanumeric());
        if !alnum && !nav.contains(&key) && !key.starts_with('F') && !key.starts_with("KP_") {
            ctrl = false;
            alt = false;
        }
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    let mut out = parts.join("+");
    if !out.is_empty() {
        out.push('+');
    }
    out.push_str(&key_alias(key));
    Some(out)
}

#[allow(dead_code)]
pub fn save_overrides(overrides: &HashMap<String, Vec<String>>) -> std::io::Result<()> {
    let path = crate::paths::keymap_file();
    std::fs::create_dir_all(path.parent().unwrap())?;
    let file = KeymapFile { schema: SCHEMA.into(), bindings: overrides.clone() };
    let text = serde_json::to_string_pretty(&file)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_normalize_like_v1() {
        assert_eq!(normalize_chord("ctrl+shift+z").unwrap(), "Ctrl+Shift+Z");
        assert_eq!(normalize_chord("Shift+Ctrl+z").unwrap(), "Ctrl+Shift+Z");
        assert_eq!(normalize_chord(" space ").unwrap(), "space");
        assert_eq!(normalize_chord("Ctrl++").unwrap(), "Ctrl+plus");
        assert_eq!(normalize_chord("+").unwrap(), "plus");
        assert!(normalize_chord("Ctrl+").is_none());
        assert!(normalize_chord("").is_none());
        assert_eq!(pretty_chord("Ctrl+Left"), "Ctrl+←");
    }

    #[test]
    fn defaults_have_no_conflicts_and_keep_v1_ids() {
        let km = Keymap::with_overrides(HashMap::new());
        assert!(km.conflicts.is_empty(), "{:?}", km.conflicts);
        assert_eq!(km.resolve("E"), Some("edit.accept"));
        assert_eq!(km.resolve("Shift+E"), Some("edit.accept_next"));
        assert_eq!(km.resolve("X"), Some("edit.toggle"));
        assert_eq!(km.resolve("P"), Some("edit.activate"));
        assert_eq!(km.resolve("S"), Some("edit.split"));
        assert_eq!(km.resolve("B"), Some("tools.cut"));
        assert_eq!(km.resolve("Shift+L"), Some("transport.skim"));
        assert_eq!(km.resolve("Ctrl+M"), Some("view.mode_montage"));
        assert_eq!(km.resolve("Ctrl+Shift+Up"), Some("montage.move_up"));
        assert_eq!(km.resolve("Ctrl+N"), Some("layers.new_lane"));
        assert_eq!(km.resolve("Shift+T"), Some("view.skip_trims"));
        assert_eq!(km.resolve("D"), Some("edit.delete"));
    }

    #[test]
    fn overrides_apply_and_conflicts_reported() {
        let mut o = HashMap::new();
        o.insert("edit.split".to_string(), vec!["E".to_string()]);
        let km = Keymap::with_overrides(o);
        assert_eq!(km.resolve("E"), Some("edit.split"));
        assert!(km.conflicts.iter().any(|(c, w, l)| c == "E" && w == "edit.split" && l == "edit.accept"));
        assert!(km.resolve("S").is_none());
    }

    #[test]
    fn altgr_on_windows_is_plain_key() {
        let m = egui::Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };
        let c = chord_from_egui(egui::Key::OpenBracket, m).unwrap();
        if cfg!(windows) {
            assert_eq!(c, "bracketleft");
        }
        let c2 = chord_from_egui(egui::Key::Z, egui::Modifiers { ctrl: true, shift: true, ..Default::default() }).unwrap();
        assert_eq!(c2, "Ctrl+Shift+Z");
    }
}
