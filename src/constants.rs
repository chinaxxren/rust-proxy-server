// 定义最大缓存个数为多少个
pub const MAX_CACHE_SIZE: usize = 20;
// 定义最大文件大小为 100MB
pub const MAX_FILE_SIZE: usize = 100 * 1024 * 1024; 
// 定义超时时间为 30 秒
pub const TIMEOUT_SECONDS: u64 = 30; 
// 定义缓存目录为 cache
pub const CACHE_DIR: &str = "cache"; 
// 定义最大重试次数为 3 次
pub const MAX_RETRIES: u32 = 3; 
// 定义重试延迟为 1000 毫秒
pub const RETRY_DELAY_MS: u64 = 1000; 

