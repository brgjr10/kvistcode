//! `kvartifact:` serves the sandboxed inline previews of HTML/SVG code blocks.
//!
//! A `data:` or `srcdoc` frame inherits the app's own CSP, which forbids inline scripts, so a
//! preview could never run anything. A custom scheme response carries its own CSP instead. The
//! page travels in the URL (base64url of the document `lib/artifact.ts` built), so there is no
//! state here and nothing to clean up. The frame stays `sandbox="allow-scripts"` (opaque origin),
//! and the CSP below allows no network at all, `ipc:` included.

use std::borrow::Cow;

use base64::Engine as _;
use tauri::http::{header, Request, Response, StatusCode};

pub const SCHEME: &str = "kvartifact";

/// Must match `ARTIFACT_CSP` in `apps/desktop/src/lib/artifact.ts`.
const ARTIFACT_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; media-src data: blob:; base-uri 'none'; form-action 'none'";

/// The source cap is 512 KiB; the SVG wrapper base64s it again and the guard adds a little.
const MAX_DOC_BYTES: usize = 1024 * 1024;

pub fn handle(req: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    match decode(req.uri().path()) {
        Some(doc) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header("Content-Security-Policy", ARTIFACT_CSP)
            .header(header::CACHE_CONTROL, "no-store")
            .header("X-Content-Type-Options", "nosniff")
            .header("Referrer-Policy", "no-referrer")
            .body(Cow::Owned(doc.into_bytes()))
            .unwrap_or_else(|_| bad()),
        None => bad(),
    }
}

fn bad() -> Response<Cow<'static, [u8]>> {
    let mut r = Response::new(Cow::Borrowed(&b""[..]));
    *r.status_mut() = StatusCode::BAD_REQUEST;
    r
}

/// The path is `/<base64url of the page>` (padding optional, maybe percent-encoded by
/// `convertFileSrc`). Anything else, too big, or not UTF-8 is refused.
fn decode(path: &str) -> Option<String> {
    let enc = path.strip_prefix('/').unwrap_or(path);
    let enc = enc.replace("%3D", "=").replace("%3d", "=");
    let enc = enc.trim_end_matches('=');
    if enc.is_empty() || enc.len() > MAX_DOC_BYTES.div_ceil(3) * 4 {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(enc)
        .ok()?;
    if bytes.len() > MAX_DOC_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enc(s: &str) -> String {
        format!(
            "/{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s)
        )
    }

    #[test]
    fn serves_the_page_with_its_own_csp() {
        let doc = "<!doctype html><p>hi ✓</p><script>1</script>";
        let req = Request::get(format!("kvartifact://localhost{}", enc(doc)))
            .body(Vec::new())
            .unwrap();
        let res = handle(&req);
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(res.headers()["content-security-policy"], ARTIFACT_CSP);
        assert!(res.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html"));
        assert_eq!(&res.body()[..], doc.as_bytes());
    }

    #[test]
    fn accepts_padding_and_percent_encoded_padding() {
        let raw = base64::engine::general_purpose::URL_SAFE.encode("ab");
        assert!(raw.ends_with('='));
        assert_eq!(decode(&format!("/{raw}")).as_deref(), Some("ab"));
        let pct = raw.replace('=', "%3D");
        assert_eq!(decode(&format!("/{pct}")).as_deref(), Some("ab"));
    }

    #[test]
    fn refuses_junk_empty_oversized_and_non_utf8() {
        assert_eq!(decode("/"), None);
        assert_eq!(decode("/not base64!"), None);
        let bin = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0xff, 0xfe, 0x00]);
        assert_eq!(decode(&format!("/{bin}")), None);
        let big = "a".repeat(MAX_DOC_BYTES + 1);
        assert_eq!(decode(&enc(&big)), None);
        let req = Request::get("kvartifact://localhost/%%%")
            .body(Vec::new())
            .unwrap();
        assert_eq!(handle(&req).status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn csp_matches_the_frontend() {
        let ts = include_str!("../../src/lib/artifact.ts");
        assert!(
            ts.contains(&format!("\"{ARTIFACT_CSP}\"")),
            "ARTIFACT_CSP differs between artifact_protocol.rs and artifact.ts"
        );
    }
}
