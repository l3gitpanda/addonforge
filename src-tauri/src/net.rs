//! Reading response bodies with a size limit.
//!
//! Every body the app reads from the network goes through here, so a server
//! (or anything between it and us) cannot make the app hold an arbitrarily
//! large reply in memory. Downloads of addon zips stream to disk instead and
//! have their own cap in `install::download`.

use serde::de::DeserializeOwned;

/// API replies: CurseForge search pages, GitHub release lists, Wago, WoWI and
/// Tukui details. The largest seen in practice is well under 1 MB.
pub const JSON_LIMIT: usize = 16 * 1024 * 1024;

/// README files, `.pkgmeta`, rendered README HTML.
pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;

/// Read a response body, stopping as soon as it passes `limit` bytes.
pub async fn read_capped(mut resp: reqwest::Response, limit: usize) -> anyhow::Result<Vec<u8>> {
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        anyhow::bail!("reply is larger than the {} limit", size_label(limit));
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if out.len() + chunk.len() > limit {
            anyhow::bail!("reply is larger than the {} limit", size_label(limit));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// A JSON reply, at most `JSON_LIMIT` bytes.
pub async fn json<T: DeserializeOwned>(resp: reqwest::Response) -> anyhow::Result<T> {
    let bytes = read_capped(resp, JSON_LIMIT).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// A text reply, at most `limit` bytes. Invalid UTF-8 is replaced, as
/// `Response::text` does.
pub async fn text(resp: reqwest::Response, limit: usize) -> anyhow::Result<String> {
    let bytes = read_capped(resp, limit).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn size_label(limit: usize) -> String {
    if limit >= 1024 * 1024 {
        format!("{} MB", limit >> 20)
    } else {
        format!("{} KB", (limit >> 10).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// One plain-HTTP reply from a local socket. Nothing leaves the machine.
    fn serve_once(body: Vec<u8>, declare_length: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let head = if declare_length {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
            } else {
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_string()
            };
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(&body);
        });
        format!("http://{addr}/")
    }

    fn fetch(url: &str, limit: usize) -> anyhow::Result<Vec<u8>> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let resp = reqwest::Client::new().get(url).send().await?;
            read_capped(resp, limit).await
        })
    }

    #[test]
    fn a_reply_within_the_limit_is_read_whole() {
        let url = serve_once(b"{\"ok\":true}".to_vec(), true);
        assert_eq!(fetch(&url, 1024).unwrap(), b"{\"ok\":true}");
    }

    #[test]
    fn a_declared_length_over_the_limit_is_refused_before_reading() {
        let url = serve_once(vec![b'x'; 4096], true);
        let err = fetch(&url, 1024).unwrap_err().to_string();
        assert!(err.contains("1 KB limit"), "{err}");
    }

    #[test]
    fn an_undeclared_length_is_cut_off_at_the_limit() {
        let url = serve_once(vec![b'x'; 64 * 1024], false);
        let err = fetch(&url, 1024).unwrap_err().to_string();
        assert!(err.contains("limit"), "{err}");
    }
}
