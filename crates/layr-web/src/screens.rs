//! Presentation fields around the canonical screenshot comparison. Never execute structure.
use super::*;

pub(crate) fn read(p: &Project, context: &probes::Context, base: &str, head: &str) -> Result<Value> {
    let subject = |id: &str| Subject { kind: "state".into(), id: id.into() };
    let behavior = probes::diff(p, &subject(base), &subject(head), context)?;
    let mut screens = Vec::new();
    for probe in behavior.probes {
        let Some(rows) = probe["changes"].as_array() else { continue };
        for row in rows.iter().filter(|row| row["kind"] == "screenshots") {
            let mut screen = row.clone();
            screen["probe"] = probe["probe"].clone();
            screen["name"] = row["screen"].clone();
            for (side, url) in [("base", "base_url"), ("head", "head_url")] {
                let path = row[format!("{side}_png")].as_str();
                if let Some(path) = path {
                    let output = probe[side]["output_state"].as_str().context("missing screen output state")?;
                    let bytes = context.outputs.get(output).and_then(|files| files.get(path)).context("missing verified PNG")?;
                    screen[url] = json!(format!("data:image/png;base64,{}", B64.encode(bytes)));
                }
            }
            screen["changed"] = json!(layr_model::screenshots::changed(row));
            screens.push(screen);
        }
    }
    Ok(json!(screens))
}
