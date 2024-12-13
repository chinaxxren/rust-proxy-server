use std::sync::Arc;
use anyhow::Result;
use hyper::Server;
use hyper::service::{make_service_fn, service_fn};
use hyper_tls::HttpsConnector;

use rust_proxy_server::cache::ProxyCache;
use rust_proxy_server::server;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let https = HttpsConnector::new();
    let client = hyper::Client::builder().build::<_, hyper::Body>(https);
    let cache = Arc::new(ProxyCache::new().await?);

    let make_svc = make_service_fn(move |_| {
        let client = client.clone();
        let cache = cache.clone();
        
        async move {
            Ok::<_, anyhow::Error>(service_fn(move |req| {
                server::handle_request(req, cache.clone(), client.clone())
            }))
        }
    });

    let addr = ([127, 0, 0, 1], 3000).into();
    let server = Server::bind(&addr).serve(make_svc);

    println!("Proxy server running on http://{}", addr);

    server.await?;
    Ok(())
}
