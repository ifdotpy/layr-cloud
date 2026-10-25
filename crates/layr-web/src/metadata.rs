use super::*;
impl Session {
    pub(super) fn project_view(&self, name: &str, now: i64) -> Result<Value> {
        let store = self.stores.get(name).context("store is locked")?;
        if let Some(snapshot) = &store.cache.heads {
            let layr_store::index::Data::Heads(heads) = &snapshot.data else {
                bail!("expected heads snapshot")
            };
            let name = store
                .model
                .projects
                .get(&store.descriptor.project)
                .and_then(|p| p.name.as_deref())
                .unwrap_or(name);
            return Ok(
                json!({"id":store.descriptor.project,"name":name,"provisional":true,"public":store.cache.public.is_some(),"index_counts":heads.counts}),
            );
        }
        let p = self.project(name)?;
        let lines = p.line_list();
        let records = p.op_log(None, usize::MAX);
        let mut probes = BTreeMap::new();
        let mut checks = Vec::new();
        for record in &records {
            let a = &record.data["attestation"];
            if a["kind"] != "check" && record.op != "check" {
                continue;
            }
            let attestation = a["attestation_id"].as_str().unwrap_or(&record.id);
            if p.is_revoked(attestation) {
                continue;
            }
            let c = record.data.get("check").unwrap_or(&record.data);
            if let (Some(name), Ok(definition)) = (
                c["probe"].as_str(),
                serde_json::from_value::<probes::Probe>(c["definition"].clone()),
            ) {
                if definition.validate(name).is_ok() {
                    let mut value = serde_json::to_value(definition)?;
                    value["name"] = json!(name);
                    probes.entry(name.to_string()).or_insert(value);
                }
            }
            let argv: Vec<String> =
                serde_json::from_value(c["argv"].clone()).unwrap_or_else(|_| {
                    c["command"]
                        .as_str()
                        .map(layr_model::protection::check_argv)
                        .unwrap_or_default()
                });
            let command = argv.join(" ");
            let name = c["probe"]
                .as_str()
                .or(c["command"].as_str())
                .unwrap_or(&command);
            if name.is_empty() {
                continue;
            }
            let mut row = json!({"name":name,"argv":argv,"machine":record.machine});
            if let Some(state) = a["subject"]["id"]
                .as_str()
                .or(c["subject"]["id"].as_str())
                .or(record.data["state"].as_str())
            {
                row["state"] = json!(state);
            }
            for field in ["exit", "duration_ms"] {
                if c[field].is_number() {
                    row[field] = c[field].clone();
                }
            }
            if let Some(value) = c["reused"].as_bool() {
                row["reused"] = json!(value);
            }
            if let Some(value) = c["log"].as_str() {
                row["log"] = json!(value);
            }
            checks.push(row);
        }
        let mut view = json!({"id":p.id,"name":p.name.as_deref().unwrap_or(&p.id),"config":p.config,"overlays":self.overlay_views(name,now)?,"checks":checks,"probes":probes.into_values().collect::<Vec<_>>(),"counts":{"lines":lines.len(),"states":p.states.len(),"changes":p.changes().len(),"refs":p.ref_list("",now).len(),"queue":p.queue_list(None).len(),"operations":records.len(),"attestations":records.iter().filter(|r|r.data.get("attestation").is_some()||matches!(r.op.as_str(),"approve"|"approve.withdraw"|"check")).count()}});
        if let Some((id, _)) = records
            .iter()
            .rev()
            .flat_map(|r| r.lines.iter())
            .find(|(_, c)| c.before.is_none() && c.after.is_some())
        {
            if let Some(line) = lines.iter().find(|l| l.id == *id) {
                view["default_line"] = json!(line.name);
            }
        }
        view["bisect_results"] = json!(p.bisect_results());
        if !p.machine.is_empty() {
            view["policy"] = json!(p.access_policy(now, false));
        }
        view["public"] = json!(store.cache.public.is_some());
        Ok(view)
    }
}
