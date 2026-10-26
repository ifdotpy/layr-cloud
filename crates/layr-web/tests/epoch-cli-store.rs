//! Real rotated CLI bytes, including file parts retained in the root epoch.
use layr_store::format;
use layr_web::Session;
use serde_json::{json, Value};
use std::{collections::{BTreeMap, BTreeSet}, fs, path::{Path, PathBuf}};

fn objects(root: &Path, prefix: &str) -> Vec<String> {
    fn walk(root: &Path, folder: &Path, out: &mut Vec<String>) {
        if !folder.exists() { return; }
        for entry in fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() { walk(root, &path, out); }
            else { out.push(path.strip_prefix(root).unwrap().to_str().unwrap().to_owned()); }
        }
    }
    let mut out=Vec::new(); walk(root, &root.join(prefix), &mut out); out.sort(); out
}
fn install(session: &mut Session, root: &Path, sources: &BTreeMap<String,String>, logical: &str, loaded: &mut BTreeSet<String>) {
    if loaded.contains(logical) { return; }
    let object=&sources[logical]; let bytes=fs::read(root.join(object)).unwrap();
    let base: Option<String>=serde_json::from_str(&session.tree_base("test",object,&bytes).unwrap()).unwrap();
    if let Some(base)=base { install(session,root,sources,&base,loaded); }
    session.install_tree("test",object,&bytes).unwrap(); loaded.insert(logical.into());
}
#[test]
#[ignore = "requires LAYR_WEB_EPOCH_FIXTURE from make-epoch-cli-fixture.sh in VM tmpfs"]
fn rotated_cli_history_files_and_current_policy_match_native() {
    let fixture=PathBuf::from(std::env::var("LAYR_WEB_EPOCH_FIXTURE").unwrap());
    assert!(fixture.starts_with("/run"));
    let root=fixture.join("archive");
    let browser: Value=serde_json::from_slice(&fs::read(fixture.join("browser.json")).unwrap()).unwrap();
    let epoch=browser["epoch"].as_str().unwrap(); let prefix=format!("project/epochs/{epoch}/");
    let discovery: BTreeMap<String,String>=objects(&root,"keys").into_iter().map(|o|{
        let line=fs::read_to_string(root.join(&o)).unwrap();(o,line)
    }).collect();
    let mut session=Session::new();
    session.unlock_recovery(&fs::read_to_string(fixture.join("person/recovery.txt")).unwrap(),&fs::read_to_string(fixture.join("person/person-view.json")).unwrap()).unwrap();
    let slot=session.slot_name().unwrap();
    session.open_epoch_store("test", &fs::read_to_string(root.join("store.json")).unwrap(),&serde_json::to_string(&discovery).unwrap(),&fs::read_to_string(root.join(format!("{prefix}store.json"))).unwrap(),&fs::read(root.join(format!("{prefix}slots/{slot}"))).unwrap()).unwrap();
    session.add_epoch_history("test","root",&fs::read(root.join(format!("{prefix}history/root.age"))).unwrap()).unwrap();
    for object in objects(&root,"records") {
        session.add_root_segment("test",&object,&fs::read(root.join(&object)).unwrap()).unwrap();
    }
    for object in objects(&root,&format!("{prefix}records")) {
        session.add_segment("test",&object,&fs::read(root.join(&object)).unwrap()).unwrap();
    }
    session.finish_records("test").unwrap();
    let projects: Value=serde_json::from_str(&session.query("test","projects","[]",0.).unwrap()).unwrap();
    assert_eq!(projects[0]["access_mode"],"private");
    assert_eq!(projects[0]["epochs"].as_array().unwrap().len(),2);
    assert_eq!(projects[0]["epochs"][1]["epoch"],epoch);
    assert_eq!(projects[0]["epochs"][1]["current"],true);
    let lines: Value=serde_json::from_slice(&fs::read(fixture.join("lines.json")).unwrap()).unwrap();
    let actual: Value=serde_json::from_str(&session.query("test","lines","[]",0.).unwrap()).unwrap();
    for line in lines.as_array().unwrap() {
        assert!(actual.as_array().unwrap().iter().any(|v|v["name"]==line["name"] && v["head"]==line["head"]));
    }
    let mut sources=BTreeMap::new();
    for (folder,scope) in [("states".to_owned(),"".to_owned()),(format!("{prefix}states"),prefix.clone())] {
        for object in objects(&root,&folder) {
            let logical=object.strip_prefix(&scope).unwrap().to_owned();sources.insert(logical,object);
        }
    }
    let mut loaded=BTreeSet::new(); let mut epochs_read=BTreeSet::new();
    for name in ["main","feature"] {
        let line=lines.as_array().unwrap().iter().find(|v|v["name"]==name).unwrap(); let state=line["head"].as_str().unwrap();
        let logical=sources.keys().find(|o|format::parse_tree_name(o).is_some_and(|(id,_)|id==state)).unwrap().clone();
        install(&mut session,&root,&sources,&logical,&mut loaded);
        let parts: Vec<String>=serde_json::from_str(&session.file_parts("test",&logical,"test.txt").unwrap()).unwrap();
        for part in parts {
            let object=&sources[&part];epochs_read.insert(object.starts_with(&prefix));
            session.add_part("test",object,&fs::read(root.join(object)).unwrap()).unwrap();
        }
        let file: Value=serde_json::from_str(&session.file("test",&logical,"test.txt").unwrap()).unwrap();
        assert_eq!(file["text"],json!(fs::read_to_string(fixture.join(format!("{name}.txt"))).unwrap()), "line {name}, state {state}");
    }
    assert_eq!(epochs_read,BTreeSet::from([false,true]),"both root and current file parts must be authenticated");
    session.lock();
}
