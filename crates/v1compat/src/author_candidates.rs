//! Read-only discovery. Invalid files remain visible; absence is the only ignored error.
use crate::{V1Result, invalid};
use std::path::{Path, PathBuf};
use tv2_domain::{Asset, SemanticLayer};

#[derive(Clone, Debug)]
pub struct Candidate {
    pub path: PathBuf,
    pub digest: Option<String>,
    pub layer: Result<SemanticLayer, String>,
}

pub fn read(path: &Path, asset: &Asset) -> V1Result<(String, SemanticLayer)> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(invalid("marcas supera 16 MiB"));
    }
    let raw = serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(&bytes))?;
    Ok((tv2_domain::digest::digest_json(&raw), crate::author::from_v1(&raw, asset)?))
}

pub fn paths(asset: &Asset, editorial: Option<&Path>, stores: &[PathBuf]) -> V1Result<Vec<PathBuf>> {
    // A fingerprint is an identity, never a user-controlled path component.
    let hash = &asset.fingerprint.hash_muestreado;
    if hash.is_empty() || !hash.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(invalid("hash de marcas no es un nombre portable"));
    }
    let media = Path::new(&asset.path);
    let mut paths = vec![media.with_extension("marcas.json")];
    if let Some(dir) = editorial {
        paths.push(dir.join("autor.marcas.json"));
        if let Some(name) = media.with_extension("marcas.json").file_name() {
            paths.push(dir.join(name));
        }
        paths.push(dir.join("marcas_store").join(format!("{hash}.marcas.json")));
    }
    paths.extend(stores.iter().map(|s| s.join(format!("{hash}.marcas.json"))));
    paths.sort();
    paths.dedup();
    Ok(paths)
}

pub fn discover(paths: &[PathBuf], asset: &Asset) -> Vec<Candidate> {
    paths
        .iter()
        .filter_map(|path| {
            if matches!(std::fs::symlink_metadata(path), Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
                return None;
            }
            let (digest, layer) = match read(path, asset) {
                Ok((digest, layer)) => (Some(digest), Ok(layer)),
                Err(e) => (None, Err(e.to_string())),
            };
            Some(Candidate { path: path.clone(), digest, layer })
        })
        .collect()
}

/// Only conflicts at the highest revision matter; the result is order independent.
pub fn preferred(candidates: &[Candidate]) -> V1Result<Option<usize>> {
    if candidates.iter().any(|c| c.layer.is_err()) {
        return Err(invalid("Hay candidatos inválidos; revisa y elige explícitamente"));
    }
    let revision = candidates.iter().filter_map(|c| c.layer.as_ref().ok().map(|l| l.revision)).max();
    let mut best: Option<usize> = None;
    for (index, candidate) in candidates.iter().enumerate() {
        if candidate.layer.as_ref().ok().map(|l| l.revision) != revision {
            continue;
        }
        if let Some(old) = best {
            if candidates[old].digest != candidate.digest {
                return Err(invalid("Marcas divergentes en la revisión más nueva"));
            }
        } else {
            best = Some(index);
        }
    }
    Ok(best)
}

/// Explicit adoption still preserves deletion history and local lane presentation.
pub fn adopt(candidate: &SemanticLayer, existing: Option<&SemanticLayer>) -> V1Result<SemanticLayer> {
    let mut layer = candidate.clone();
    if let Some(old) = existing {
        if old.locked || old.deleted {
            return Err(invalid("Desbloquea la capa antes de resolver marcas"));
        }
        if old.asset_id != layer.asset_id {
            return Err(invalid("Marcas de otro medio"));
        }
        if layer.items.iter().any(|i| old.deleted_item_ids.contains(&i.item_id)) {
            return Err(invalid("El candidato resucitaría marcas borradas"));
        }
        layer.layer_id = old.layer_id.clone();
        layer.visible = old.visible;
        layer.color = old.color.clone();
        layer.deleted_item_ids.extend(old.deleted_item_ids.iter().cloned());
        layer.deleted_item_ids.extend(old.items.iter().filter(|i| layer.item(&i.item_id).is_none()).map(|i| i.item_id.clone()).collect::<Vec<_>>());
        layer.deleted_item_ids.sort();
        layer.deleted_item_ids.dedup();
        layer.revision = layer.revision.max(old.revision);
    }
    Ok(layer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn discovery_resolution_revalidation_and_tombstones() {
        let dir = tempfile::tempdir().unwrap();
        let mut asset = tv2_domain::commands::tests_support::fake_video("a", 20);
        asset.path = dir.path().join("ñ media.mp4").to_string_lossy().into();
        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        let paths = paths(&asset, None, &[store]).unwrap();
        let raw = json!({"schema":1,"timebase":"media_elapsed_v1","revision":2,"next_id":2,"fuente":asset.fingerprint,"marcas":[{"id":"m0001","tipo":"punto","t":1.0}]});
        for p in &paths {
            std::fs::write(p, raw.to_string()).unwrap();
        }
        let candidates = discover(&paths, &asset);
        assert!(preferred(&candidates).unwrap().is_some());
        let mut changed = raw.clone();
        changed["marcas"][0]["t"] = json!(2.0);
        std::fs::write(&paths[1], changed.to_string()).unwrap();
        assert!(preferred(&discover(&paths, &asset)).is_err());
        changed["revision"] = json!(3);
        std::fs::write(&paths[1], changed.to_string()).unwrap();
        assert_eq!(preferred(&discover(&paths, &asset)).unwrap(), Some(1));
        assert_ne!(read(&paths[1], &asset).unwrap().0, candidates[1].digest.clone().unwrap());
        let mut old = candidates[0].layer.clone().unwrap();
        old.deleted_item_ids.push("m0001".into());
        old.items.clear();
        assert!(adopt(candidates[1].layer.as_ref().unwrap(), Some(&old)).is_err());
        std::fs::write(&paths[0], "{incomplete").unwrap();
        assert!(discover(&paths, &asset)[0].layer.is_err());
        assert!(preferred(&discover(&paths, &asset)).is_err());
        asset.fingerprint.hash_muestreado = "../escape".into();
        assert!(super::paths(&asset, None, &[]).is_err());
    }
}
