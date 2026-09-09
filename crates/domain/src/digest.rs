//! Canonicalización JSON compatible con `editorial_io.digest_json` (V1):
//! `json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))`
//! y SHA-256 del UTF-8.
//!
//! Diferencias de Python que hay que reproducir:
//! - claves ordenadas por comparación de cadenas Unicode (orden de code points);
//! - floats con `repr` de Python (shortest round-trip; `1e-05`, `1e+16`, `.0` obligatorio);
//! - enteros sin `.0`; booleanos `true/false`; `null`;
//! - cadenas con escapes JSON mínimos (`"`, `\\`, control chars como `\uXXXX`
//!   salvo `\n \r \t \b \f`), sin escapar no-ASCII.

use serde_json::Value;
use sha2::{Digest, Sha256};

pub fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_value(value, &mut out);
    out
}

pub fn digest_json(value: &Value) -> String {
    let text = canonical_json(value);
    hex::encode(Sha256::digest(text.as_bytes()))
}

pub fn digest_object_without(value: &Value, excluded: &[&str]) -> String {
    let Some(map) = value.as_object() else {
        return digest_json(value);
    };
    let mut keys: Vec<_> = map.keys().filter(|k| !excluded.contains(&k.as_str())).collect();
    keys.sort();
    let mut out = String::from("{");
    for (n, key) in keys.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        write_string(key, &mut out);
        out.push(':');
        write_value(&map[*key], &mut out);
    }
    out.push('}');
    hex::encode(Sha256::digest(out.as_bytes()))
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(f) = n.as_f64() {
                out.push_str(&python_float_repr(f));
            }
        }
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.chars().cmp(b.chars()));
            out.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(k, out);
                out.push(':');
                write_value(&map[*k], out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `repr(float)` de Python: representación más corta que hace round-trip,
/// notación científica si el exponente < -4 o >= 16, siempre con `.0` si es entero.
pub fn python_float_repr(f: f64) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    // Rust `{}` produce la representación más corta con round-trip (Grisu/Ryu), sin exponente.
    // Obtenemos dígitos y exponente decimal a partir de `{:e}`.
    let sci = format!("{:e}", f); // p. ej. "1.5e-5", "-2e16"
    let (mantissa, exp) = sci.split_once('e').expect("formato científico");
    let exp: i32 = exp.parse().expect("exponente");
    let negative = mantissa.starts_with('-');
    let mantissa = mantissa.trim_start_matches('-');
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();
    // valor = 0.d1d2d3... × 10^(exp+1)
    let decpt = exp + 1;
    let mut s = String::new();
    if negative {
        s.push('-');
    }
    if (-4..16).contains(&exp) {
        if decpt <= 0 {
            s.push_str("0.");
            for _ in 0..(-decpt) {
                s.push('0');
            }
            s.push_str(&digits);
        } else if (decpt as usize) >= digits.len() {
            s.push_str(&digits);
            for _ in 0..(decpt as usize - digits.len()) {
                s.push('0');
            }
            s.push_str(".0");
        } else {
            s.push_str(&digits[..decpt as usize]);
            s.push('.');
            s.push_str(&digits[decpt as usize..]);
        }
    } else {
        s.push_str(&digits[..1]);
        if digits.len() > 1 {
            s.push('.');
            s.push_str(&digits[1..]);
        }
        s.push('e');
        if exp < 0 {
            s.push('-');
        } else {
            s.push('+');
        }
        s.push_str(&format!("{:02}", exp.abs()));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn float_repr_matches_python() {
        let cases: &[(f64, &str)] = &[
            (1.0, "1.0"),
            (0.5, "0.5"),
            (12.345, "12.345"),
            (1e-5, "1e-05"),
            (0.0001, "0.0001"),
            (1e16, "1e+16"),
            (1234567890123456.0, "1234567890123456.0"),
            (-2.5, "-2.5"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100.0"),
            (3.0e-7, "3e-07"),
            (1.5e300, "1.5e+300"),
        ];
        for (f, expected) in cases {
            assert_eq!(python_float_repr(*f), *expected, "{f}");
        }
    }

    #[test]
    fn canonical_json_matches_python_dumps() {
        let v = json!({"b": [1, 2.5, "ñandú", null, true], "a": {"z": 1e-05, "y": "tab\there"}, "á": 0});
        // Python: json.dumps(v, ensure_ascii=False, sort_keys=True, separators=(",",":"))
        // -> '{"a":{"y":"tab\\there","z":1e-05},"b":[1,2.5,"ñandú",null,true],"á":0}'
        assert_eq!(canonical_json(&v), "{\"a\":{\"y\":\"tab\\there\",\"z\":1e-05},\"b\":[1,2.5,\"ñandú\",null,true],\"á\":0}");
    }

    #[test]
    fn digests_match_python_hashlib_goldens() {
        // Valores calculados con Python 3.14: hashlib.sha256 sobre
        // json.JSONEncoder(ensure_ascii=False, sort_keys=True, separators=(",",":")).iterencode(v)
        assert_eq!(digest_json(&json!({"a": 1})), "015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862");
        let v = json!({"b": [1, 2.5, "ñandú", null, true], "a": {"z": 1e-05, "y": "tab	here"}, "á": 0});
        assert_eq!(digest_json(&v), "650e87972cd44038a50712f84a8a5c5ff5537775cb13a208dda192cf9a43964f");
        let v = json!({"schema":"editorial-layer/1","items":[{"item_id":"item-1","ranges":[{"t_ini":1.5,"t_fin":2.0}],"state":"proposed","edited":true}],"revision":3,"x":1e16,"y":0.30000000000000004,"z":-0.0,"w":123456789012345678i64});
        assert_eq!(
            canonical_json(&v),
            "{\"items\":[{\"edited\":true,\"item_id\":\"item-1\",\"ranges\":[{\"t_fin\":2.0,\"t_ini\":1.5}],\"state\":\"proposed\"}],\"revision\":3,\"schema\":\"editorial-layer/1\",\"w\":123456789012345678,\"x\":1e+16,\"y\":0.30000000000000004,\"z\":-0.0}"
        );
        assert_eq!(digest_json(&v), "a567490787a8c8dccbbc1fae370c65c46a8774a860b8485401a815718a84a55a");
    }
}
