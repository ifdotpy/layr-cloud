use super::*;
type Files = BTreeMap<String, (String, String, format::Entry, Option<String>)>;
fn same(
    a: Option<&(String, String, format::Entry, Option<String>)>,
    b: Option<&(String, String, format::Entry, Option<String>)>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.2 .0 == b.2 .0 && a.2 .1 == b.2 .1 && a.2 .2 == b.2 .2,
        _ => false,
    }
}
impl Session {
    pub(super) fn change_states(
        &self,
        name: &str,
        id: &str,
        from: &str,
        to: &str,
    ) -> Result<(layr_model::StateRec, layr_model::StateRec)> {
        let versions = self.project(name)?.change_versions(id);
        let pick = |label: &str| -> Result<layr_model::StateRec> {
            if let Some(index) = label
                .strip_prefix('v')
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|n| n.checked_sub(1))
            {
                return versions
                    .get(index)
                    .cloned()
                    .context("change version is absent");
            }
            versions
                .iter()
                .find(|s| s.id == label)
                .cloned()
                .context("change version is absent")
        };
        Ok((pick(from)?, pick(to)?))
    }
    pub(super) fn change_inputs(
        &self,
        name: &str,
        id: &str,
        from: &str,
        to: &str,
    ) -> Result<Value> {
        let (a, b) = self.change_states(name, id, from, to)?;
        let states: BTreeSet<_> = [
            Some(a.id),
            a.parents.first().cloned(),
            Some(b.id),
            b.parents.first().cloned(),
        ]
        .into_iter()
        .flatten()
        .collect();
        Ok(json!(states))
    }
    fn state_files(&self, name: &str, state: Option<&str>) -> Result<Files> {
        let Some(state) = state else {
            return Ok(BTreeMap::new());
        };
        let store = self.stores.get(name).context("store is locked")?;
        let object = store
            .trees
            .keys()
            .find(|object| format::parse_tree_name(object).is_some_and(|v| v.0 == state))
            .context("change version tree is unavailable")?;
        self.view_files(name, object)
    }
    fn path_change(
        &self,
        path: &str,
        before: &Files,
        after: &Files,
    ) -> Result<Option<layr_model::diff::FileDiff>> {
        let a = before.get(path);
        let b = after.get(path);
        if same(a, b) {
            return Ok(None);
        }
        let side=|source:&(String,String,format::Entry,Option<String>)|->Result<layr_model::diff::Side> {
            let big=(source.2.1>layr_model::diff::MAX_TEXT).then_some(source.2.1);
            Ok(layr_model::diff::Side{path:path.into(),mode:u32::from_str_radix(&source.2.0,8)?,data:if big.is_some(){vec![]}else{self.read_file(&source.0,&source.1,path)?},big})
        };
        Ok(Some(layr_model::diff::FileDiff {
            old: a.map(side).transpose()?,
            new: b.map(side).transpose()?,
        }))
    }
    pub(super) fn change_diff(
        &self,
        name: &str,
        id: &str,
        from: &str,
        to: &str,
        path: &str,
    ) -> Result<Value> {
        let (a, b) = self.change_states(name, id, from, to)?;
        let ap = self.state_files(name, a.parents.first().map(String::as_str))?;
        let ah = self.state_files(name, Some(&a.id))?;
        let bp = self.state_files(name, b.parents.first().map(String::as_str))?;
        let bh = self.state_files(name, Some(&b.id))?;
        if !path.is_empty() {
            if !format::safe_path(path) {
                bail!("invalid interdiff path");
            }
            let old = self
                .path_change(path, &ap, &ah)?
                .into_iter()
                .collect::<Vec<_>>();
            let new = self
                .path_change(path, &bp, &bh)?
                .into_iter()
                .collect::<Vec<_>>();
            let mut output = serde_json::to_value(layr_model::interdiff::from_changes(
                id, from, to, &old, &new,
            ))?;
            output["compared"] = json!(true);
            return Ok(output);
        }
        let mut files = Vec::new();
        for path in ap
            .keys()
            .chain(ah.keys())
            .chain(bp.keys())
            .chain(bh.keys())
            .collect::<BTreeSet<_>>()
        {
            if path == ".git" || path.starts_with(".git/") {
                continue;
            }
            let old = !same(ap.get(path), ah.get(path));
            let new = !same(bp.get(path), bh.get(path));
            if !old && !new || same(ap.get(path), bp.get(path)) && same(ah.get(path), bh.get(path))
            {
                continue;
            }
            files.push(json!({"path":path,"status":if !old{"added"}else if !new{"removed"}else{"modified"}}));
        }
        Ok(json!({"change_id":id,"from":from,"to":to,"files":files,"compared":false}))
    }
}
