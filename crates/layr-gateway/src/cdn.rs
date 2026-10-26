//! Only create-once ciphertext may leave the direct gateway route.

pub fn cacheable(key: &str) -> bool {
    if key.len() > 1024 || key.split('/').any(|s| s.is_empty() || matches!(s, "." | "..")) {
        return false;
    }
    let (epoch, inner) = match crate::epochs::split(key) {
        Some((_, inner)) => (true, inner),
        None => (false, key),
    };
    use layr_store::format;
    crate::server::object_name(inner)
        && (format::parse_tree_name(inner).is_some()
            || format::parse_data_name(inner).is_some()
            || format::parse_segment_name(inner).is_some()
            || inner.starts_with("index/paths/")
            || inner.starts_with("index/graph/")
            || layr_store::format::release_file(inner)
            || (epoch && inner.starts_with("history/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "01a177f1-4bd0-7029-b488-6f00793f9798";
    #[test]
    fn allowlist_accepts_only_valid_immutable_object_classes() {
        for key in [format!("states/{ID}/12345678.tree.age"),
            format!("states/{ID}/12345678.0000.data.age"),
            layr_store::format::segment_name(ID, 1, 2), format!("index/paths/{ID}.age"),
            format!("index/graph/{ID}.age"), format!("files/{ID}.age")] {
            assert!(cacheable(&key), "{key}");
            assert!(cacheable(&format!("project/epochs/{ID}/{key}")));
        }
        assert!(cacheable(&format!("project/epochs/{ID}/history/root.age")));
        for key in ["store.json".into(), "public.json".into(), "keys/epoch.json".into(),
            format!("slots/machine-{ID}.age"), format!("index/heads/{ID}/1.age"),
            format!("requests/{ID}.json"), format!("claims/{ID}.json"),
            "history/root.age".into(), "states/../key.age".into(),
            format!("project/epochs/invalid/states/{ID}/12345678.tree.age"),
            format!("project/epochs/{ID}/project/epochs/{ID}/files/{ID}.age"),
            format!("overlays/{ID}/keys/{ID}.json"), "records/a.age".into()] {
            assert!(!cacheable(&key), "{key}");
        }
    }
}
