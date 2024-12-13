use anyhow::Result;
use hyper::{Body, Client, Request, header::HeaderMap};
use hyper_tls::HttpsConnector;

use crate::utils::fetch_with_retry;

pub async fn get_total_size(
    client: &Client<HttpsConnector<hyper::client::HttpConnector>>,
    req: &Request<Body>,
) -> Result<Option<u64>> {
    let head_req = Request::builder()
        .method(hyper::Method::HEAD)
        .uri(req.uri())
        .body(Body::empty())?;

    let resp = fetch_with_retry(client, &head_req).await?;
    
    // 先检查 Content-Range
    if let Some(range) = resp.headers().get(hyper::header::CONTENT_RANGE) {
        if let Ok(range_str) = range.to_str() {
            if let Some(total_size) = range_str.split('/').last() {
                if let Ok(size) = total_size.parse::<u64>() {
                    return Ok(Some(size));
                }
            }
        }
    }
    
    // 再检查 Content-Length
    if let Some(length) = resp.headers().get(hyper::header::CONTENT_LENGTH) {
        if let Some(expected_len) = length.to_str().ok().and_then(|v| v.parse::<u64>().ok()) {
            return Ok(Some(expected_len));
        }
    }

    // If no headers available, return None
    Ok(None)
}

pub fn check_response_complete(headers: &HeaderMap, content_length: u64) -> bool {
    if let Some(content_range) = headers.get(hyper::header::CONTENT_RANGE) {
        if let Ok(range_str) = content_range.to_str() {
            if let Some(total_size) = range_str.split('/').last() {
                if let Ok(total) = total_size.parse::<u64>() {
                    return content_length == total;
                }
            }
        }
        false
    } else {
        headers
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .map(|len| content_length == len)
            .unwrap_or(true)
    }
}
