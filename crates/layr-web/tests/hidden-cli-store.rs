//! Fixture: run make-hidden-cli-fixture.sh as root in layr-dev with CLI b1a23c1.
use layr_store::{format, person};
use layr_web::Session;
use serde_json::{json, Value};
use std::{fs, path::Path};

fn objects(root: &Path, prefix: &str) -> Vec<String> {
    fn visit(root: &Path, directory: &Path, found: &mut Vec<String>) {
        if !directory.exists() {
            return;
        }
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, found)
            } else {
                found.push(path.strip_prefix(root).unwrap().to_str().unwrap().into());
            }
        }
    }
    let mut found = Vec::new();
    visit(root, &root.join(prefix), &mut found);
    found.sort();
    found
}
fn records(session: &mut Session, alias: &str, root: &Path) {
    for object in objects(root, "records") {
        session
            .add_segment(alias, &object, &fs::read(root.join(&object)).unwrap())
            .unwrap();
    }
    session.finish_records(alias).unwrap();
}
fn open(fixture: &Path, label: &str, archive: &str) -> Session {
    let root = fixture.join(archive);
    let mut session = Session::new();
    let view = fs::read_to_string(fixture.join(format!("{label}-view.json"))).unwrap();
    let recovery = fs::read_to_string(fixture.join(format!("{label}-paper"))).unwrap();
    session.unlock_recovery(&recovery, &view).unwrap();
    let descriptor = fs::read_to_string(root.join("store.json")).unwrap();
    let slot = session.slot_name().unwrap();
    session
        .open_store(
            "test",
            &descriptor,
            &fs::read(root.join("slots").join(&slot)).unwrap(),
        )
        .unwrap();
    records(&mut session, "test", &root);
    let all = objects(&root, "overlays");
    for object in &all {
        if !object.contains("/keys/") || !object.ends_with(".age") {
            continue;
        }
        let parts: Vec<_> = object.split('/').collect();
        let prefix = format!("overlays/{}/epochs/{}", parts[1], &parts[3][..36]);
        let slot_object = format!("{prefix}/slots/{slot}");
        if !all.contains(&slot_object) {
            continue;
        }
        let slot_bytes = fs::read(root.join(slot_object)).unwrap();
        if !session.overlay_slot_opens(&slot_bytes) {
            continue;
        }
        let alias = session
            .open_hidden_overlay(
                "test",
                object,
                &slot_bytes,
                &fs::read(root.join(object)).unwrap(),
            )
            .unwrap();
        if !alias.is_empty() {
            records(&mut session, &alias, &root.join(prefix));
        }
    }
    session
}
fn query(session: &Session, method: &str, args: Value) -> String {
    if method == "overlays" {
        let projects: Value =
            serde_json::from_str(&session.query("", "projects", "[]", 0.0).unwrap()).unwrap();
        return projects[0]["overlays"].to_string();
    }
    session
        .query("test", method, &args.to_string(), 0.0)
        .unwrap()
}
fn install(session: &mut Session, alias: &str, root: &Path, object: &str) {
    let bytes = fs::read(root.join(object)).unwrap();
    let base: Option<String> =
        serde_json::from_str(&session.tree_base(alias, object, &bytes).unwrap()).unwrap();
    if let Some(base) = base {
        install(session, alias, root, &base);
    }
    session.install_tree(alias, object, &bytes).unwrap();
}

#[test]
#[ignore = "requires a real CLI fixture; set LAYR_WEB_HIDDEN_FIXTURE"]
fn cli_hidden_overlay_reader_and_non_reader_are_isolated() {
    let fixture = std::env::var("LAYR_WEB_HIDDEN_FIXTURE").expect("fixture folder");
    let fixture = Path::new(&fixture);
    let mut reader = open(fixture, "reader", "archive");
    let mut other = open(fixture, "other", "archive");
    let mut absent = open(fixture, "other", "without-hidden");
    let rows: Vec<Value> = serde_json::from_str(&query(&reader, "overlays", json!([]))).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "hidden-browser-secret");
    assert_eq!(rows[0]["hidden"], true);
    assert_eq!(rows[0]["readable"], true);
    let mut cli: Value =
        serde_json::from_str(&fs::read_to_string(fixture.join("overlays.json")).unwrap()).unwrap();
    // CLI renders UID 0 through Linux's account database. The browser retains
    // the signed canonical principal, as it does for visible overlays.
    for reader in cli[0]["readers"].as_array_mut().unwrap() {
        if reader == "root" {
            *reader = json!("user:0");
        }
    }
    for field in ["name", "hidden", "mode", "owner", "readers"] {
        assert_eq!(rows[0][field], cli[0][field], "CLI overlay field {field}");
    }
    for method in [
        "overlays", "projects", "lines", "opLog", "stores", "machines",
    ] {
        assert_eq!(
            query(&other, method, json!([])),
            query(&absent, method, json!([])),
            "{method}"
        );
    }
    assert_eq!(query(&other, "overlays", json!([])), "[]");
    let lines: Vec<Value> = serde_json::from_str(&query(&reader, "lines", json!([]))).unwrap();
    let state = lines.iter().find(|l| l["name"] == "main").unwrap()["head"]
        .as_str()
        .unwrap();
    let layers: Vec<Value> =
        serde_json::from_str(&query(&reader, "layers", json!([state]))).unwrap();
    assert_eq!(
        layers.len(),
        1,
        "hidden follow layers must come from the model"
    );
    assert_eq!(layers[0]["overlay"], "hidden-browser-secret");
    assert_eq!(
        query(&other, "layers", json!([state])),
        query(&absent, "layers", json!([state]))
    );
    let root = fixture.join("archive");
    let layer = &layers[0];
    let manifest = objects(&root, "overlays")
        .into_iter()
        .find(|k| k.contains("/keys/"))
        .unwrap();
    let parts: Vec<_> = manifest.split('/').collect();
    let layer_root = root.join(format!("overlays/{}/epochs/{}", parts[1], &parts[3][..36]));
    let layer_state = layer["state"].as_str().unwrap();
    let layer_object = objects(&layer_root, "states")
        .into_iter()
        .find(|k| format::parse_tree_name(k).is_some_and(|v| v.0 == layer_state))
        .unwrap();
    let layer_alias = layer["name"].as_str().unwrap();
    install(&mut reader, layer_alias, &layer_root, &layer_object);
    let object = objects(&root, "states")
        .into_iter()
        .find(|k| format::parse_tree_name(k).is_some_and(|v| v.0 == state))
        .unwrap();
    install(&mut reader, "test", &root, &object);
    install(&mut other, "test", &root, &object);
    install(
        &mut absent,
        "test",
        &fixture.join("without-hidden"),
        &object,
    );
    let tree: Value = serde_json::from_str(&reader.tree("test", &object, "").unwrap()).unwrap();
    assert!(tree["entries"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["path"] == "hidden.txt"));
    assert_eq!(
        other.tree("test", &object, "").unwrap(),
        absent.tree("test", &object, "").unwrap()
    );
    for part in serde_json::from_str::<Vec<String>>(
        &reader
            .file_parts(layer_alias, &layer_object, "hidden.txt")
            .unwrap(),
    )
    .unwrap()
    {
        reader
            .add_part(
                layer_alias,
                &part,
                &fs::read(layer_root.join(&part)).unwrap(),
            )
            .unwrap();
    }
    let file: Value =
        serde_json::from_str(&reader.file("test", &object, "hidden.txt").unwrap()).unwrap();
    assert_eq!(file["text"], "hidden browser bytes\n");
    // Even a well-formed slot for someone else must not expose a bad manifest error.
    let reader_view: Value =
        serde_json::from_str(&fs::read_to_string(fixture.join("reader-view.json")).unwrap())
            .unwrap();
    let reader_slot = person::slot_name(
        reader_view["login"].as_str().unwrap(),
        reader_view["recipient"].as_str().unwrap(),
    )
    .unwrap();
    let foreign_slot = fs::read(layer_root.join("slots").join(reader_slot)).unwrap();
    assert!(!other.overlay_slot_opens(&foreign_slot));
    assert_eq!(
        other
            .open_hidden_overlay("test", &manifest, &foreign_slot, b"invalid manifest")
            .unwrap(),
        ""
    );
    assert_eq!(query(&other, "overlays", json!([])), "[]");
    reader.lock();
    other.lock();
    absent.lock();
}
