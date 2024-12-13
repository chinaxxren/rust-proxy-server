use anyhow::Result;
use bytes::Bytes;
use futures::StreamExt;
use hyper::{Body, Client, Request, Response, StatusCode};
use hyper_tls::HttpsConnector;
use std::sync::Arc;

use crate::cache::{CacheEntry, CacheMeta, ProxyCache};
use crate::constants::;
use crate::handler::{check_response_complete, get_total_size, handle_range_request};
use crate::utils::{self, fetch_with_retry, generate_cache_key, parse_range};

pub async fn handle_request(
    req: Request<Body>,
    cache: Arc<ProxyCache>,
    client: Client<HttpsConnector<hyper::client::HttpConnector>>,
) -> Result<Response<Body>> {
    // 生成缓存键
    let cache_key = generate_cache_key(req.uri());

    // 检查缓存是否存在
    if let Some(cached_entry) = cache.get(&cache_key).await {
        // 检查是否有范围请求
        if let Some(range_header) = req.headers().get(hyper::header::RANGE) {
            // 处理范围请求
            if let Ok(range_str) = range_header.to_str() {
                // 解析范围请求
                if let Some(range) = parse_range(range_str) {
                    // 处理范围请求
                    return handle_range_request(
                        range,
                        cached_entry,
                        req,
                        client,
                        cache,
                        cache_key,
                    )
                    .await;
                }
            }
        
        // 如果没有范围请求，检查是否完整
        } else if cached_entry.meta.is_complete {
            // 返回完整的缓存响应
            let response = Response::builder()
                .status(StatusCode::OK)
                .header(
                    hyper::header::CONTENT_TYPE,
                    cached_entry
                        .meta
                        .content_type
                        .parse::<hyper::header::HeaderValue>()
                        .unwrap(),
                )
                .body(Body::from(cached_entry.content))?;
            return Ok(response);
        
        // 处理不完整的缓存
        } else {
            // 获取缓存长度
            let cached_len = cached_entry.content.len() as u64;

            // 获取总资源大小
            let total_size = if let Some(size) = cached_entry.meta.total_size {
                size
            } else {
                get_total_size(&client, &req).await?.unwrap_or(0)
            };

            if total_size > 0 {
                if cached_len >= total_size {
                    // 缓存实际上已完成
                    let response = Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            hyper::header::CONTENT_TYPE,
                            cached_entry
                                .meta
                                .content_type
                                .parse::<hyper::header::HeaderValue>()
                                .unwrap(),
                        )
                        .body(Body::from(cached_entry.content))?;
                    return Ok(response);
                } else {
                    // 获取剩余部分
                    let mut client_req = Request::builder()
                        .method(req.method())
                        .uri(req.uri())
                        .header(
                            hyper::header::RANGE,
                            format!("bytes={}-{}", cached_len, total_size - 1),
                        )
                        .body(Body::empty())?;

                    *client_req.headers_mut() = req.headers().clone();

                    // 获取剩余部分
                    let resp = fetch_with_retry(&client, &client_req).await?;
                    if resp.status() == StatusCode::PARTIAL_CONTENT {
                        let mut remaining_data = Vec::new();
                        let mut stream = resp.into_body();

                        while let Some(chunk) = stream.next().await {
                            let chunk = chunk?;
                            remaining_data.extend_from_slice(&chunk);

                            // 检查合并后的总大小是否超过限制
                            if (cached_len + remaining_data.len() as u64) > total_size as u64 {
                                // 如果超过限制，返回原始的完整请求
                                return fetch_and_cache_full_response(
                                    &client, req, cache, cache_key,
                                )
                                .await;
                            }
                        }

                        // 合并缓存数据和新数据
                        let mut complete_data = cached_entry.content.to_vec();
                        complete_data.extend_from_slice(&remaining_data);

                        // 更新缓存
                        let new_cache_entry = CacheEntry {
                            content: Bytes::from(complete_data.clone()),
                            meta: CacheMeta {
                                content_type: cached_entry.meta.content_type.clone(),
                                is_complete: true,
                                total_size: Some(total_size),
                            },
                        };
                        cache.set(cache_key, new_cache_entry).await?;

                        // 返回完整响应
                        let response = Response::builder()
                            .status(StatusCode::OK)
                            .header(
                                hyper::header::CONTENT_TYPE,
                                cached_entry
                                    .meta
                                    .content_type
                                    .parse::<hyper::header::HeaderValue>()
                                    .unwrap(),
                            )
                            .body(Body::from(complete_data))?;
                        return Ok(response);
                    }
                }
            }
        }
    }

    // 如果上述所有情况都不满足，获取根据请求的 range 情况来获取数据
    fetch_and_cache_full_response(&client, req, cache, cache_key).await
}

// 获取根据请求的 range 情况来获取数据
async fn fetch_and_cache_full_response(
    client: &Client<HttpsConnector<hyper::client::HttpConnector>>,
    req: Request<Body>,
    cache: Arc<ProxyCache>,
    cache_key: String,
) -> Result<Response<Body>> {
    let resp = fetch_with_retry(&client, &req).await?;
    let status = resp.status();
    let headers = resp.headers().clone();

    if status.is_success() {
        // 处理成功响应
        let content_type = headers
            .get(hyper::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_string();

        let mut body = Vec::new();
        let mut stream = resp.into_body();

        // 读取响应主体
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            body.extend_from_slice(&chunk);

            // 检查是否超过最大文件大小
            if body.len() as u64 > MAX_FILE_SIZE as u64 {
                // 如果主体大小超过限制，则返回而不缓存
                let response = Response::builder().status(status).body(Body::from(body))?;
                return Ok(response);
            }
        }

        // 检查是否完成
        let is_complete = check_response_complete(&headers, body.len() as u64);

        // 获取总资源大小
        let total_size = get_total_size(&client, &req)
            .await?
            .or_else(|| Some(body.len() as u64));

        // 缓存响应
        cache
            .set(
                cache_key,
                CacheEntry {
                    content: Bytes::from(body.clone()),
                    meta: CacheMeta {
                        content_type,
                        is_complete,
                        total_size,
                    },
                },
            )
            .await?;

        // 构建响应
        let mut response = Response::builder().status(status).body(Body::from(body))?;
        *response.headers_mut() = headers;
        Ok(response)
    } else {
        // 处理失败响应
        let mut response = Response::builder().status(status).body(resp.into_body())?;
        *response.headers_mut() = headers;
        Ok(response)
    }
}
