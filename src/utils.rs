use anyhow::Result;
use hyper::{body, Body, Client, Request, Response};
use hyper_tls::HttpsConnector;
use sha2::{Digest, Sha256};
use std::{mem, time::Duration};
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
        let cloned_req = clone_request(req).await.unwrap();
            
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


// 支持get请求 和 head请求
pub async fn clone_request(req: &Request<Body>) -> Result<Request<Body>> {
   
   let mut new_req = Request::new(Body::empty());
   *new_req.method_mut() = req.method().clone();
   *new_req.uri_mut() = req.uri().clone();
   *new_req.headers_mut() = req.headers().clone();
   *new_req.version_mut() = req.version().clone();
   
   Ok(new_req)
}

pub async fn clone_request_with_body(req: &mut Request<Body>) -> Result<Request<Body>> {
    // 使用 `std::mem::take` 将 body 从 `req` 中移出
   let body = mem::take(req.body_mut());

   let body_bytes = body::to_bytes(body).await?;

   let mut new_req = Request::new(Body::from(body_bytes));
   *new_req.method_mut() = req.method().clone();
   *new_req.uri_mut() = req.uri().clone();
   *new_req.headers_mut() = req.headers().clone();
   *new_req.version_mut() = req.version().clone();
   
   Ok(new_req)
}

// #[tokio::main]
// async fn main() {
//     // 假设我们有一个初始的 Request<Body>
//     let initial_req = Request::builder()
//         .method("GET")
//         .uri("http://example.com")
//         .body(Body::from("Hello, world!"))
//         .unwrap();

//     // 克隆请求
//     let cloned_req = clone_request_async(&initial_req).await;

//     println!("Original request: {:?}", initial_req);
//     println!("Cloned request: {:?}", cloned_req);
// }