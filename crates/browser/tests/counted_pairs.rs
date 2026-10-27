//! One report per colour pair and one per outer panel, each counting the
//! rest (taste calls r14-t2 and r14-t5), over
//! `tests/fixtures/antipatterns/low-contrast-counted-pairs.html` and
//! `nested-cards.html`, rendered by an installed browser. Skips cleanly when
//! there is none.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};

use impeccable_browser::BrowserEngine;
use impeccable_detect::engines::{ScanOptions, UrlEngine};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/antipatterns")
}

fn serve() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || handle(stream));
        }
    });
    port
}

fn handle(mut stream: TcpStream) {
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).unwrap_or(0);
    let request = String::from_utf8_lossy(&buf[..n]);
    let path = request
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let (status, body) = match std::fs::read(fixtures_dir().join(path.trim_start_matches('/'))) {
        Ok(body) => ("200 OK", body),
        Err(_) => ("404 Not Found", b"missing".to_vec()),
    };
    let head = format!(
        "HTTP/1.0 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body);
    let _ = stream.flush();
}

fn engine() -> Option<BrowserEngine> {
    let env: HashMap<String, String> = std::env::vars().collect();
    if impeccable_browser::discovery::find_browser(&env).is_err() {
        eprintln!("skip: no installed browser found");
        return None;
    }
    Some(BrowserEngine::new(env))
}

/// `(rule, selector, snippet)` for every finding of `rules` on one fixture.
fn findings(engine: &BrowserEngine, port: u16, fixture: &str, rules: &[&str]) -> Vec<(String, String, String)> {
    let url = format!("http://127.0.0.1:{port}/{fixture}");
    engine
        .detect_url(&url, &ScanOptions::default())
        .expect("scan")
        .into_iter()
        .filter(|f| rules.contains(&f.antipattern.as_str()))
        .map(|f| {
            let selector = f.extras.get("selector").and_then(|s| s.as_str()).unwrap_or("").to_string();
            (f.antipattern, selector, f.snippet)
        })
        .collect()
}

#[test]
fn a_colour_pair_reports_once_and_counts_the_rest() {
    let Some(engine) = engine() else { return };
    let port = serve();
    let found = findings(&engine, port, "low-contrast-counted-pairs.html", &["low-contrast"]);
    let got: Vec<(&str, &str)> = found.iter().map(|(_, sel, snippet)| (sel.as_str(), snippet.as_str())).collect();
    assert_eq!(
        got,
        vec![
            // Three names in divs, the full pass.
            ("#flag-name-first", "1.9:1 (need 4.5:1) — text #bbbbbb on #ffffff (and 2 more)"),
            // Three links, the SAFE_TAGS path.
            ("#flag-share-first", "2.3:1 (need 4.5:1) — text #ababab on #ffffff (and 2 more)"),
            // A price span and a name div: one pair across both paths.
            ("#flag-shop-price", "2.8:1 (need 4.5:1) — text #e15c99 on #f9e5ef (and 1 more)"),
            // The ignored first note waives itself only.
            ("#flag-note-second", "2.1:1 (need 4.5:1) — text #b4b4b4 on #ffffff (and 1 more)"),
            // A pair worn once carries no count.
            ("#flag-lone-caption", "1.8:1 (need 4.5:1) — text #c2c2c2 on #ffffff"),
        ],
        "{found:#?}"
    );
}

#[test]
fn an_outer_panel_reports_once_and_counts_its_cards() {
    let Some(engine) = engine() else { return };
    let port = serve();
    let found = findings(&engine, port, "nested-cards.html", &["nested-cards"]);
    let on = |id: &str| -> Vec<&str> {
        found.iter().filter(|(_, sel, _)| sel == id).map(|(_, _, s)| s.as_str()).collect()
    };
    // pgportal.gov.in's FAQ panel: the first row reports and counts two.
    assert_eq!(on("#flag-faq-first"), ["Card inside card (and 2 more)"], "{found:#?}");
    assert!(on("#flag-faq-second").is_empty() && on("#flag-faq-third").is_empty(), "{found:#?}");
    // An inline ignore on the first tile waives that tile only.
    assert!(on("#pass-tiles-ignored").is_empty(), "{found:#?}");
    assert_eq!(on("#flag-tiles-second"), ["Card inside card (and 1 more)"], "{found:#?}");
    assert!(on("#flag-tiles-third").is_empty(), "{found:#?}");
    // A panel with one inner card is unchanged.
    assert_eq!(on("#flag-outlined-inner"), ["Card inside card"], "{found:#?}");
}
