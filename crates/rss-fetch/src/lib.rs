fn default_client() -> Result<reqwest::blocking::Client, Box<dyn std::error::Error>> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?)
}

pub fn fetch_feed_bytes(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let client = default_client()?;
    fetch_feed_bytes_with_client(&client, url)
}

pub fn fetch_feed_bytes_with_client(client: &reqwest::blocking::Client, url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let resp = client.get(url).send()?;
    let bytes = resp.bytes()?.to_vec();
    Ok(bytes)
}

/// Outcome of a conditional HTTP fetch.
pub enum FetchOutcome {
    /// Server returned new content (200 OK).
    Modified {
        bytes: Vec<u8>,
        etag: Option<String>,
        last_modified: Option<String>,
    },
    /// Server returned 304 Not Modified — no new content.
    NotModified,
}

/// Fetch a feed URL with conditional GET using ETag / Last-Modified headers.
/// Falls back to a normal fetch if no cache headers are provided.
pub fn fetch_feed_conditional(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchOutcome, Box<dyn std::error::Error>> {
    let client = default_client()?;
    fetch_feed_conditional_with_client(&client, url, etag, last_modified)
}

pub fn fetch_feed_conditional_with_client(
    client: &reqwest::blocking::Client,
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> Result<FetchOutcome, Box<dyn std::error::Error>> {
    let mut req = client.get(url);

    if let Some(etag_val) = etag {
        req = req.header("If-None-Match", etag_val);
    }
    if let Some(lm_val) = last_modified {
        req = req.header("If-Modified-Since", lm_val);
    }

    let resp = req.send()?;

    if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }

    let resp_etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let resp_last_modified = resp
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let bytes = resp.bytes()?.to_vec();
    Ok(FetchOutcome::Modified {
        bytes,
        etag: resp_etag,
        last_modified: resp_last_modified,
    })
}
