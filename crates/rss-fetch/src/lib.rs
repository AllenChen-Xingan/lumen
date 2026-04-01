pub fn fetch_feed_bytes(url: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let resp = reqwest::blocking::get(url)?;
    let bytes = resp.bytes()?.to_vec();
    Ok(bytes)
}
