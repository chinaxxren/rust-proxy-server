use anyhow::Result;
use hyper::{Body, Client, Request, Response};
use hyper_tls::HttpsConnector;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::time::sleep;

use crate::constants::{MAX_RETRIES, RETRY_DELAY_MS, TIMEOUT_SECONDS};

pub fn generate_cache_key(uri: &hyper::Uri) -> String {
    let mut hasher = Sha256::new();
    hasher.update(uri.to_string().as_bytes());
    hex::encode(hasher.finalize())
}

pub fn parse_range(range: &str) -> Option<(u64, u64)> {
    let range = range.trim_start_matches("bytes=");
    let mut parts = range.split('-');
    let start = parts.next()?.parse().ok()?;
    let end = parts.next()?.parse().ok()?;
    Some((start, end))
}

pub async fn fetch_with_retry(
    client: &Client<HttpsConnector<hyper::client::HttpConnector>>,
    req: &Request<Body>,
) -> Result<Response<Body>> {
    let mut retries = 0;
    loop {
        //
        let mut builder = Request::builder()
            .method(req.method())
            .uri(req.uri())
            .version(req.version());
            
        // Copy all headers
        for (name, value) in req.headers() {
            builder = builder.header(name, value);
        }
        
        // TODO 支持get请求 和 head请求
        let cloned_req = builder.body(Body::empty())?;
            
        match tokio::time::timeout(
            Duration::from_secs(TIMEOUT_SECONDS),
            client.request(cloned_req),
        )
        .await
        {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(e)) => {
                if retries >= MAX_RETRIES {
                    return Err(e.into());
                }
            }
            Err(_) => {
                if retries >= MAX_RETRIES {
                    return Err(anyhow::anyhow!("Request timed out after {} retries", MAX_RETRIES));
                }
            }
        }
        retries += 1;
        sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
    }
}
