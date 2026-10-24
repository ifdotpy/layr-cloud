use super::*;
use std::{fs, path::Path};

fn objects(root: &Path, prefix: &str) -> Vec<String> {
    fn visit(root: &Path, path: &Path, out: &mut Vec<String>) {
        if !path.exists() {
            return;
        }
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(root, &path, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_str().unwrap().into());
            }
        }
    }
    let mut out = Vec::new();
    visit(root, &root.join(prefix), &mut out);
    out.sort();
    out
}
fn records(session: &mut Session, name: &str, root: &Path) {
    for object in objects(root, "records") {
        session
            .add_segment(name, &object, &fs::read(root.join(&object)).unwrap())
            .unwrap();
    }
    session.finish_records(name).unwrap();
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
fn assert_details(session: &mut Session, root: &Path, oracle: &Path, overlay: &str) {
    let rows = session.overlay_views("test", 0).unwrap();
    let before = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == overlay)
        .unwrap();
    assert_eq!(before["readable"], true);
    assert!(
        before.get("paths").is_none(),
        "no guessed empty paths before tree verification"
    );
    assert!(before.get("files").is_none());
    let inputs: Value = serde_json::from_str(&session.overlay_inputs("test").unwrap()).unwrap();
    let alias = format!("test::{overlay}");
    let input = inputs
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == alias)
        .unwrap();
    assert_eq!(input["state"], before["state"]);
    let state = input["state"].as_str().unwrap();
    let own = session.stores[&alias].parent.as_ref().unwrap().1.prefix();
    let own = root.join(own);
    let object = objects(&own, "states")
        .into_iter()
        .find(|object| format::parse_tree_name(object).is_some_and(|v| v.0 == state))
        .unwrap();
    install(session, &alias, &own, &object);
    let rows = session.overlay_views("test", 0).unwrap();
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == overlay)
        .unwrap();
    // TODO(cli-overlay-json): replace the raw signed database oracle with
    // `layr overlay log --ops --json` and `layr overlay show --json` after that task merges.
    let signed = fs::read_to_string(oracle.join("signed-history.jsonl")).unwrap();
    for line in signed.lines() {
        let raw = record::parse_line(line, None).unwrap();
        let r: Record = serde_json::from_str(raw.get()).unwrap();
        record::verify_line(line.as_bytes(), &session.stores["test"].keys[&r.machine]).unwrap();
    }
    let history: Vec<Record> =
        serde_json::from_slice(&fs::read(oracle.join("history.json")).unwrap()).unwrap();
    assert_eq!(row["history"], json!(history), "own CLI signed operations");
    let epochs: Value =
        serde_json::from_slice(&fs::read(oracle.join("epochs.json")).unwrap()).unwrap();
    assert_eq!(
        row["epochs"], epochs,
        "own key sequence order and signed epoch metadata"
    );
    let patterns: Value =
        serde_json::from_slice(&fs::read(oracle.join("patterns.json")).unwrap()).unwrap();
    assert_eq!(row["patterns"], patterns, "CLI routed patterns");
    let log: Vec<Value> = fs::read_to_string(oracle.join("log.json"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!log.is_empty());
    assert_eq!(row["state"], log[0]["id"], "CLI overlay log head");
    let paths: Vec<_> = fs::read_to_string(oracle.join("paths.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    assert_eq!(row["paths"], json!(paths), "CLI overlay ls-files");
    assert_eq!(
        row["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["path"].as_str().unwrap())
            .collect::<Vec<_>>(),
        paths
    );
    assert!(row["files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["state"] == row["state"]
            && f["size"].is_u64()
            && f.get("text").is_none()
            && f.get("base64").is_none()));
    assert!(
        session.stores[&alias].parts.is_empty(),
        "detail metadata never needs data parts"
    );
}

#[test]
#[ignore = "requires LAYR_WEB_VISIBLE_STORE, LAYR_WEB_VISIBLE_PAPER, LAYR_WEB_VISIBLE_ORACLE"]
fn visible_cli_overlay_details_match_log_signed_records_and_paths() {
    let root = std::env::var("LAYR_WEB_VISIBLE_STORE").unwrap();
    let root = Path::new(&root);
    let paper = fs::read_to_string(std::env::var("LAYR_WEB_VISIBLE_PAPER").unwrap()).unwrap();
    let opener = Zeroizing::new(keys::identity_from_paper(&paper).unwrap());
    let descriptor: format::Descriptor =
        serde_json::from_slice(&fs::read(root.join("store.json")).unwrap()).unwrap();
    let identity = Zeroizing::new(
        keys::open_slot(&fs::read(root.join("slots/paper.age")).unwrap(), &opener).unwrap(),
    );
    let (mut session, _, _, _) = crate::tests::fixture();
    session.index = person::Index {
        vaults: vec![person::Pin::of_store(&descriptor)],
    };
    let recipient = keys::recipient_of(session.person.as_ref().unwrap()).unwrap();
    session
        .open_store(
            "test",
            &serde_json::to_string(&descriptor).unwrap(),
            &keys::seal_slot(&identity, &recipient).unwrap(),
        )
        .unwrap();
    records(&mut session, "test", root);
    for object in objects(root, "overlays")
        .into_iter()
        .filter(|p| p.contains("/keys/") && p.ends_with(".json"))
    {
        session
            .overlay_key("test", &object, &fs::read(root.join(&object)).unwrap())
            .unwrap();
    }
    let rows = session.overlay_views("test", 0).unwrap();
    let unread = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["name"] == "secrets")
        .unwrap();
    for field in ["history", "epochs", "patterns", "paths", "files", "state"] {
        assert!(unread.get(field).is_none(), "unread private field {field}");
    }
    assert_eq!(session.overlay_inputs("test").unwrap(), "[]");
    let parent = &session.stores["test"];
    let published = parent.published["secrets"].clone();
    let authority = parent.authorities["secrets"].clone();
    // The golden CLI fixture has a legitimate paper reader, but no person reader.
    // Open that existing CLI slot in test setup; all records/trees still pass the
    // same Session signature, authority, binding and tree verification as the browser.
    let identity = Zeroizing::new(
        keys::open_slot(
            &fs::read(root.join(published.prefix()).join("slots/paper.age")).unwrap(),
            &opener,
        )
        .unwrap(),
    );
    assert_eq!(
        keys::recipient_of(&identity).unwrap(),
        published.key.recipient
    );
    session.stores.insert(
        "test::secrets".into(),
        Store {
            identity,
            descriptor: format::Descriptor {
                format: 2,
                project: published.key.overlay.clone(),
                recipient: published.key.recipient.clone(),
            },
            authorities: BTreeMap::new(),
            published: BTreeMap::new(),
            parent: Some(("test".into(), published.clone(), authority)),
            segments: BTreeMap::new(),
            keys: BTreeMap::new(),
            model: Model::default(),
            trees: BTreeMap::new(),
            parts: BTreeMap::new(),
        },
    );
    records(
        &mut session,
        "test::secrets",
        &root.join(published.prefix()),
    );
    let oracle = std::env::var("LAYR_WEB_VISIBLE_ORACLE").unwrap();
    assert_details(&mut session, root, Path::new(&oracle), "secrets");
}

#[test]
#[ignore = "requires LAYR_WEB_HIDDEN_FIXTURE and LAYR_WEB_HIDDEN_ORACLE"]
fn hidden_cli_overlay_details_match_log_signed_records_and_paths() {
    let fixture = std::env::var("LAYR_WEB_HIDDEN_FIXTURE").unwrap();
    let fixture = Path::new(&fixture);
    let root = fixture.join("archive");
    let mut session = Session::new();
    let words = fs::read_to_string(fixture.join("reader-paper")).unwrap();
    let view = fs::read_to_string(fixture.join("reader-view.json")).unwrap();
    session.unlock_recovery(&words, &view).unwrap();
    let slot = session.slot_name().unwrap();
    session
        .open_store(
            "test",
            &fs::read_to_string(root.join("store.json")).unwrap(),
            &fs::read(root.join("slots").join(&slot)).unwrap(),
        )
        .unwrap();
    records(&mut session, "test", &root);
    let object = objects(&root, "overlays")
        .into_iter()
        .find(|p| p.contains("/keys/") && p.ends_with(".age"))
        .unwrap();
    let pieces: Vec<_> = object.split('/').collect();
    let prefix = format!("overlays/{}/epochs/{}", pieces[1], &pieces[3][..36]);
    let alias = session
        .open_hidden_overlay(
            "test",
            &object,
            &fs::read(root.join(&prefix).join("slots").join(slot)).unwrap(),
            &fs::read(root.join(&object)).unwrap(),
        )
        .unwrap();
    records(&mut session, &alias, &root.join(&prefix));
    let oracle = std::env::var("LAYR_WEB_HIDDEN_ORACLE").unwrap();
    assert_details(
        &mut session,
        &root,
        Path::new(&oracle),
        "hidden-browser-secret",
    );
}

#[test]
fn epochs_follow_signed_sequence_despite_equal_or_skewed_times() {
    let mut keys = Vec::new();
    for (seq, time, epoch) in [
        (2, 300, "first"),
        (3, 300, "second"),
        (4, 100, "third"),
        (5, 500, "first"),
    ] {
        let mut record = Record::new("overlay.key");
        record.seq = seq;
        record.time = time;
        record.machine = "machine".into();
        record.id = format!("record-{seq}");
        record.data = json!({"epoch":epoch});
        keys.push(record);
    }
    keys.reverse();
    let epochs = key_epochs(&keys);
    assert_eq!(
        epochs
            .iter()
            .map(|e| (e["id"].as_str().unwrap(), e["number"].as_u64().unwrap()))
            .collect::<Vec<_>>(),
        [("first", 1), ("second", 2), ("third", 3)]
    );
    assert_eq!(epochs[0]["time"], 300);
    assert_eq!(epochs[2]["time"], 100);
    assert!(epochs.iter().all(|e| e.get("reason").is_none()));
}

#[test]
fn routed_patterns_come_from_readable_own_settings() {
    let (mut session, _, _, _) = crate::tests::fixture();
    let parent = session.stores["test"].descriptor.project.clone();
    let overlay = "01a12290-0000-7000-8000-000000000004";
    let machine = "01a12290-0000-7000-8000-000000000002";
    let make = |project: &str, data: Value| {
        let mut record = Record::new("config");
        record.id = "01a12290-0000-7000-8000-000000000006".into();
        record.project = project.into();
        record.machine = machine.into();
        record.seq = 1;
        record.time = 1;
        record.data = data;
        record
    };
    let public = make(
        &parent,
        json!({"config":{"overlay.secret":{"patterns":["public-fallback/**"]}}}),
    );
    session.stores.get_mut("test").unwrap().model = Model::from_records([public], machine).unwrap();
    let own = make(
        overlay,
        json!({"config":{"overlay.secret":{"patterns":["secrets/**","*.key"]}}}),
    );
    let identity = Zeroizing::new(keys::generate_identity());
    let recipient = keys::recipient_of(&identity).unwrap();
    session.stores.insert(
        "test::secret".into(),
        Store {
            identity,
            descriptor: format::Descriptor {
                format: 2,
                project: overlay.into(),
                recipient,
            },
            parent: None,
            authorities: BTreeMap::new(),
            published: BTreeMap::new(),
            segments: BTreeMap::new(),
            keys: BTreeMap::new(),
            model: Model::from_records([own], machine).unwrap(),
            trees: BTreeMap::new(),
            parts: BTreeMap::new(),
        },
    );
    let mut row = json!({});
    session.overlay_details("test", "secret", &mut row).unwrap();
    assert_eq!(row["patterns"], json!(["secrets/**", "*.key"]));
    assert!(row.get("state").is_none());
    assert!(row.get("paths").is_none());
    assert!(session.stores["test::secret"].parts.is_empty());
}
