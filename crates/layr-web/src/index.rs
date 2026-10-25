//! Indexes are authenticated caches; all history is reconciled against signed sources.
use super::*;
use layr_store::{index as store_index, public};

pub(super) const DISCARDED: &str = "Index discarded. Showing verified records.";
#[derive(Default)]
pub(super) struct Cache {
    pub heads: Option<store_index::Index>,
    candidates: Vec<store_index::Index>,
    pub paths: BTreeMap<String, store_index::Paths>,
    pub graphs: BTreeMap<String, store_index::Graph>,
    pub trusted: BTreeMap<String, VerifyingKey>,
    pub note: Option<String>,
    pub public: Option<public::Manifest>,
}
#[wasm_bindgen]
impl Session {
    /// A public manifest is not a trust root. finish_records verifies its writer.
    pub fn open_public_store(&mut self, name: &str, descriptor: &str, manifest: &[u8], slot: &[u8]) -> std::result::Result<String, JsValue> {
        self.add_public_store(name, descriptor, manifest, slot).map_err(js)
    }
    pub fn add_heads(&mut self, name: &str, object: &str, bytes: &[u8]) -> std::result::Result<(), JsValue> {
        self.cache_heads(name, object, bytes).map_err(js)
    }
    pub fn use_heads(&mut self, name: &str) -> std::result::Result<bool, JsValue> {
        self.select_heads(name).map_err(js)
    }
    pub fn discard_index(&mut self, name: &str) {
        if let Some(store) = self.stores.get_mut(name) {
            store.cache.heads = None;
            store.cache.candidates.clear();
            store.cache.paths.clear();
            store.cache.graphs.clear();
            store.cache.note = Some(DISCARDED.into());
        }
    }
    pub fn add_history_index(&mut self, name: &str, object: &str, bytes: &[u8]) -> std::result::Result<(), JsValue> {
        self.cache_history(name, object, bytes).map_err(js)
    }
    /// Called after loading history trees, before selecting file versions to fetch.
    pub fn history_versions(&mut self, name: &str, state: &str, path: &str) -> std::result::Result<String, JsValue> {
        self.prepare_history(name, state).and_then(|_| self.file_history_view(name, state, path, false)).and_then(encoded).map_err(js)
    }
}
impl Session {
    fn add_public_store(&mut self, name: &str, descriptor: &str, bytes: &[u8], slot: &[u8]) -> Result<String> {
        let descriptor: format::Descriptor = serde_json::from_str(descriptor)?;
        let manifest = public::Manifest::parse(bytes, &descriptor)?;
        let identity = Zeroizing::new(manifest.open(slot)?);
        let project = descriptor.project.clone();
        self.stores.insert(name.into(), Store {
            cache: Cache { public: Some(manifest), ..Default::default() },
            authorities: BTreeMap::new(), published: BTreeMap::new(), parent: None,
            identity, descriptor, segments: BTreeMap::new(), keys: BTreeMap::new(),
            model: Model::default(), trees: BTreeMap::new(), parts: BTreeMap::new(),
        });
        Ok(project)
    }
    fn cache_heads(&mut self, name: &str, object: &str, bytes: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let index = store_index::open(&store.identity, bytes, object, &store.descriptor.project, &store.cache.trusted)?;
        if !matches!(index.data, store_index::Data::Heads(_)) { bail!("expected heads snapshot"); }
        store.cache.candidates.push(index);
        Ok(())
    }
    fn select_heads(&mut self, name: &str) -> Result<bool> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        store.cache.heads = store_index::newest(&store.cache.candidates)?.cloned();
        store.cache.candidates.clear();
        Ok(store.cache.heads.is_some())
    }
    pub(super) fn index_answer(&self, name: &str, method: &str, _args: &Value) -> Result<Option<Value>> {
        let Some(store) = self.stores.get(name) else { return Ok(None) };
        let Some(snapshot) = &store.cache.heads else { return Ok(None) };
        let store_index::Data::Heads(heads) = &snapshot.data else { bail!("expected heads snapshot") };
        Ok(Some(match method {
            "lines" => json!(heads.lines),
            "refs" => json!(heads.refs),
            "openChanges" => json!(heads.changes),
            "copies" => json!([]),
            // A first page never invents missing state metadata from the index summary.
            "log" | "queue" | "opLog" => json!([]),
            "layers" => json!([]),
            _ => return Ok(None),
        }))
    }
    fn cache_history(&mut self, name: &str, object: &str, bytes: &[u8]) -> Result<()> {
        let store = self.stores.get_mut(name).context("store is locked")?;
        let index = store_index::open(&store.identity, bytes, object, &store.descriptor.project, &store.cache.trusted)?;
        match index.data {
            store_index::Data::Paths(p) => { store.cache.paths.insert(p.state.clone(), p); }
            store_index::Data::Graph(g) => { store.cache.graphs.insert(g.state.clone(), g); }
            _ => bail!("expected file history index"),
        }
        Ok(())
    }
    pub(super) fn history_states(&self, name: &str, start: &str) -> Result<Vec<String>> {
        let p = self.project(name)?;
        let mut at = start;
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        loop {
            if !seen.insert(at.to_string()) { bail!("state history has a loop"); }
            let state = p.state(at).context("state is absent")?;
            out.push(at.to_string());
            let Some(parent) = state.parents.first() else { break };
            at = parent;
        }
        Ok(out)
    }
    fn state_tree<'a>(&'a self, name: &str, state: &str) -> Result<&'a format::Tree> {
        self.stores.get(name).context("store is locked")?.trees.iter()
            .find(|(object, _)| format::parse_tree_name(object).is_some_and(|v| v.0 == state))
            .map(|(_, (_, tree))| tree).context("history tree is absent")
    }
    fn prepare_history(&mut self, name: &str, start: &str) -> Result<()> {
        let states = self.history_states(name, start)?;
        let p = self.project(name)?;
        let mut paths = BTreeMap::new();
        let mut graphs = BTreeMap::new();
        let empty = format::Tree { files: BTreeMap::new() };
        for id in states.iter().rev() {
            let state = p.state(id).context("state is absent")?;
            let tree = self.state_tree(name, id)?;
            let parent = state.parents.first().map(|id| self.state_tree(name, id)).transpose()?.unwrap_or(&empty);
            paths.insert(id.clone(), layr_model::index::paths(id, parent, tree));
            let graph = layr_model::index::graph(state, |id| graphs.get(id).cloned().context("ancestor graph is absent"))?;
            graphs.insert(id.clone(), graph);
        }
        let store = self.stores.get_mut(name).context("store is locked")?;
        let mismatch = states.iter().any(|id| {
            store.cache.paths.get(id).is_some_and(|cached| Some(cached) != paths.get(id)) ||
            store.cache.graphs.get(id).is_some_and(|cached| Some(cached) != graphs.get(id))
        });
        if mismatch { store.cache.note = Some(DISCARDED.into()); }
        // Use the derived values even when a cache object is missing or wrong.
        store.cache.paths = paths;
        store.cache.graphs = graphs;
        Ok(())
    }
    pub(super) fn file_history_view(&self, name: &str, start: &str, path: &str, blame: bool) -> Result<Value> {
        if !format::safe_path(path) { bail!("invalid file path"); }
        let store = self.stores.get(name).context("store is locked")?;
        let history = layr_model::index::History { project: self.project(name)?, paths: &store.cache.paths, graphs: &store.cache.graphs };
        if !blame { return Ok(json!(history.file_history(start, path)?)); }
        let lines = history.blame(start, path, |state, path| {
            let tree = self.state_tree(name, state)?;
            tree.files.get(path).map(|entry| reader::file(&store.identity, entry, &store.parts)).transpose()
        })?;
        Ok(json!({"lines":lines,"states":history.file_history(start,path)?}))
    }
}
