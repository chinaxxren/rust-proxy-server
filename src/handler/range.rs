use std::sync::Arc;
use anyhow::Result;
use bytes::Bytes;
use futures::StreamExt;
use hyper::{Body, Client, Request, Response, StatusCode};
use hyper_tls::HttpsConnector;

use crate::cache::{CacheEntry, CacheMeta, ProxyCache};
use crate::constants::MAX_FILE_SIZE;
use crate::utils::fetch_with_retry;

pub async fn handle_range_request(
    range: (u64, u64),
    cached_entry: CacheEntry,
    req: Request<Body>,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
    cache: Arc<ProxyCache>,
    cache_key: String,
) -> Result<Response<Body>> {
    let cached_len = cached_entry.content.len() as u64;
    let (start, end) = range;

    // 请求的范围在缓存范围内
    if end <= cached_len {
        
        // 请求的范围已完全缓存
        let slice = cached_entry.content.slice(start as usize..end as usize + 1);
        
        // 构建响应
        let response = Response::builder()
            .status(StatusCode::PARTIAL_CONTENT)
            .header(
                hyper::header::CONTENT_TYPE,
                cached_entry.meta.content_type.parse::<hyper::header::HeaderValue>().unwrap(),
            )
            .header(
                hyper::header::CONTENT_RANGE,
                format!("bytes {}-{}/{}", start, end, cached_len),
            )
            .body(Body::from(slice))?;
        Ok(response)
    } else {
        // 需要获取额外的数据
        let mut client_req = Request::builder()
            .method(req.method())
            .uri(req.uri())
            .header(
                hyper::header::RANGE,
                format!("bytes={}-{}", cached_len, end),
            )
            .body(Body::empty())?;

        *client_req.headers_mut() = req.headers().clone();

        // 从源服务器获取数据
        let resp = fetch_with_retry(&client, &client_req).await?;
        
        // 如果响应状态码为部分内容，则将数据与缓存数据合并后返回
        if resp.status() == StatusCode::PARTIAL_CONTENT {
            let mut body = Vec::new();
            let mut stream = resp.into_body();
            
            // 读取响应主体
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                body.extend_from_slice(&chunk);
            }
            
            // 将数据与缓存数据合并
            let mut new_content = cached_entry.content.to_vec();
            // 合并数据
            new_content.extend_from_slice(&body);
            
            let content_type = cached_entry.meta.content_type.clone();
            
            // 更新缓存
            if new_content.len()  <= MAX_FILE_SIZE {
                
                // 缓存数据未超过最大文件大小，直接更新缓存
                cache.set(
                    cache_key,
                    CacheEntry {
                        content: Bytes::from(new_content.clone()),
                        meta: CacheMeta {
                            content_type: content_type.clone(),
                            is_complete: end == new_content.len() as u64 - 1,
                            total_size: Some(new_content.len() as u64),
                        },
                    },
                ).await?;
            }
            
            // 构建响应
            let response_slice = new_content[start as usize..=end as usize].to_vec();
            
            // 构建响应
            let response = Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(
                    hyper::header::CONTENT_TYPE,
                    content_type.parse::<hyper::header::HeaderValue>().unwrap(),
                )
                .header(
                    hyper::header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, new_content.len()),
                )
                .body(Body::from(response_slice))?;
            Ok(response)
        } else {
            // 响应状态码不是部分内容，直接返回
            Ok(resp)
        }
    }
}
