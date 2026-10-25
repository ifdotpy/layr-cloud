//! Same-origin built site. API routes never fall back to a page.
use crate::server::Reply;
use std::path::{Component, Path};

pub fn serve(root: &Path, method: &str, parts: &[String]) -> Reply {
    if !matches!(method, "GET" | "HEAD") {
        return Reply::error(405, "site requires GET or HEAD");
    }
    let name = if parts == [String::new()] {
        "index.html".to_string()
    } else {
        if parts.iter().any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || part.contains(['/', '\\'])
                || Path::new(part)
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
        }) {
            return Reply::error(400, "invalid site path");
        }
        parts.join("/")
    };
    let Ok(root) = root.canonicalize() else {
        return Reply::error(503, "site is unavailable");
    };
    let Ok(path) = root.join(&name).canonicalize() else {
        return Reply::error(404, "no such site file");
    };
    if !path.starts_with(&root) || !path.is_file() {
        return Reply::error(404, "no such site file");
    }
    let media = match path.extension().and_then(|v| v.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "wasm" => "application/wasm",
        "woff2" => "font/woff2",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" => "application/json",
        "txt" => "text/plain; charset=utf-8",
        _ => return Reply::error(404, "unsupported site file"),
    };
    let Ok(body) = std::fs::read(path) else {
        return Reply::error(503, "site is unavailable");
    };
    let mut reply = Reply::bytes(200, body, media);
    for (name, value) in &mut reply.headers {
        if name == "Content-Security-Policy" {
            *value = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self'; font-src 'self'; connect-src 'self'; worker-src 'self'; img-src 'self' blob: data:; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'".into();
        }
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serves_assets_and_refuses_paths_outside_site() {
        let root = std::env::temp_dir().join(format!("layr-site-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), "<title>Layr</title>").unwrap();
        std::fs::write(root.join("assets/model.wasm"), b"\0asm").unwrap();
        std::os::unix::fs::symlink("/etc/passwd", root.join("escape.txt")).unwrap();
        assert_eq!(serve(&root, "GET", &["".into()]).status, 200);
        let wasm = serve(&root, "GET", &["assets".into(), "model.wasm".into()]);
        assert_eq!(wasm.status, 200);
        assert!(wasm
            .headers
            .contains(&("Content-Type".into(), "application/wasm".into())));
        for parts in [
            vec!["..".into(), "etc".into()],
            vec!["assets/../index.html".into()],
        ] {
            assert_eq!(serve(&root, "GET", &parts).status, 400);
        }
        assert_eq!(serve(&root, "GET", &["escape.txt".into()]).status, 404);
        assert_eq!(serve(&root, "GET", &["missing.js".into()]).status, 404);
        assert_eq!(serve(&root, "POST", &["".into()]).status, 405);
        assert_eq!(serve(&root, "HEAD", &["".into()]).status, 200);
        std::fs::remove_dir_all(root).unwrap();
    }
}
