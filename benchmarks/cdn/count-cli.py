#!/usr/bin/env python3
"""Make an isolated measurement copy; do not change a core branch or log bearer URLs."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import sys

COUNTER = r'''
// Measurement copy only. Keep headers, URLs, aliases, object IDs and credentials out of evidence.
trait CountedSend { fn counted_send(self) -> std::result::Result<Response, reqwest::Error>; }
impl CountedSend for reqwest::blocking::RequestBuilder {
    fn counted_send(self) -> std::result::Result<Response, reqwest::Error> {
        let (client, request) = self.build_split(); let request = request?;
        let parts: Vec<_> = request.url().path().split('/').filter(|part| !part.is_empty()).collect();
        let class = |parts: &[&str]| {
            let parts = if parts.starts_with(&["project", "epochs"]) { parts.get(3..).unwrap_or(&[]) } else { parts };
            match parts.first().copied().unwrap_or("") {
                "states"=>"states", "records"=>"records", "files"=>"files", "slots"=>"slots",
                "keys"=>"keys", "history"=>"history", "requests"=>"requests", "claims"=>"claims",
                "store.json"=>"store.json", "public.json"=>"public.json",
                "index"=>match parts.get(1).copied().unwrap_or("") {"heads"=>"index-heads","paths"=>"index-paths","graph"=>"index-graph",_=>"index-other"},
                _=>"other",
            }
        };
        let route = if parts.first()==Some(&"o") { format!("cdn:{}",class(parts.get(2..).unwrap_or(&[]))) }
            else if parts.starts_with(&["v1","p"]) {
                match parts.get(3).copied().unwrap_or("") {
                    "objects" if parts.len()==4 => {
                        let prefix=request.url().query_pairs().find(|(name,_)|name=="prefix").map(|(_,value)|value.into_owned()).unwrap_or_default();
                        format!("gateway:list:{}",class(&prefix.split('/').collect::<Vec<_>>()))
                    },
                    "objects"=>format!("gateway:objects:{}",class(parts.get(4..).unwrap_or(&[]))),
                    "cdn"=>"gateway:cdn-batch".into(),
                    "token"=>"gateway:token".into(),"challenge"=>"gateway:challenge".into(),
                    "store.json"=>"gateway:store.json".into(),"slots"=>"gateway:slots".into(),_=>"gateway:other".into(),
                }
            } else { "other".into() };
        let object = if parts.first()==Some(&"o") { Some(parts.get(2..).unwrap_or(&[]).join("/")) }
            else if parts.starts_with(&["v1","p"]) && parts.get(3)==Some(&"objects") && parts.len()>4 {Some(parts[4..].join("/"))} else {None};
        let object_hash=object.as_ref().map(|key|{use sha2::Digest;hex::encode(sha2::Sha256::digest(key.as_bytes()))});
        let signing=request.headers().get("x-layr-cdn").is_some();
        let batch_keys=if route=="gateway:cdn-batch" {request.body().and_then(|body|body.as_bytes()).and_then(|body|serde_json::from_slice::<serde_json::Value>(body).ok()).and_then(|body|body["keys"].as_array().map(Vec::len))} else {None};
        let method=request.method().as_str().to_string();
        let started=std::time::Instant::now(); let result=client.execute(request);
        if let Ok(path)=std::env::var("LAYR_COUNT_HTTP_FILE") {
            static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _guard=LOCK.lock().expect("measurement lock");
            let response=result.as_ref().ok();
            let cache=response.and_then(|r|r.headers().get("x-cache")).and_then(|v|v.to_str().ok()).map(|v|match v {
                "Hit from cloudfront"=>"hit","Miss from cloudfront"=>"miss","RefreshHit from cloudfront"=>"refresh-hit",_=>"other"});
            let row=serde_json::json!({"method":method,"route":route,"status":response.map(|r|r.status().as_u16()),
                "objectHash":object_hash,"singleSigning":signing,"batchKeys":batch_keys,
                "signedMetadataResponse":response.and_then(|r|r.headers().get("content-type")).and_then(|v|v.to_str().ok()).is_some_and(|v|v.starts_with("application/vnd.layr.cdn+json")),
                "declaredBytes":response.and_then(|r|r.content_length()),"cache":cache,"responseMs":started.elapsed().as_millis()});
            if let Ok(mut file)=std::fs::OpenOptions::new().create(true).append(true).mode(0o600).open(path) {
                use std::io::Write; let _=file.write_all(format!("{row}\n").as_bytes());
            }
        }
        result
    }
}
'''


def main():
    if sys.version_info < (3, 12):
        raise SystemExit("Python 3.12 or newer is required for safe archive extraction")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        raise ValueError('Measurement output must be a new directory')
    revision = subprocess.check_output(['git','-C',str(args.repo),'rev-parse',args.revision+'^{commit}'],text=True).strip()
    raw = subprocess.check_output(['git','-C',str(args.repo),'archive',revision])
    args.output.mkdir()
    with tarfile.open(fileobj=io.BytesIO(raw)) as archive:
        archive.extractall(args.output, filter='data')
    path = args.output/'src/store_remote/gateway.rs'
    source = path.read_text()
    count = source.count('.send()')
    if count < 5:
        raise ValueError('Gateway request implementation differs; inspect before instrumenting')
    changed = source.replace('.send()', '.counted_send()')
    position = changed.index('const OBJECT_LIMIT:')
    changed = changed[:position] + COUNTER + '\n' + changed[position:]
    path.write_text(changed)
    receipt = {'revision':revision,'archiveSha256':hashlib.sha256(raw).hexdigest(),
               'gatewayBeforeSha256':hashlib.sha256(source.encode()).hexdigest(),
               'gatewayAfterSha256':hashlib.sha256(changed.encode()).hexdigest(),'sendCalls':count}
    (args.output/'measurement-source.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(receipt))


if __name__ == '__main__': main()
