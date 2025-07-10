use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub response_times: Vec<Duration>,
    pub token_usage: TokenUsage,
    pub memory_usage: MemoryUsage,
    pub error_rates: ErrorRates,
    pub throughput: ThroughputMetrics,
    pub cache_hit_rates: CacheHitRates,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub average_input_tokens: f64,
    pub average_output_tokens: f64,
    pub token_cost: f64,
    pub tokens_per_request: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    pub current_memory: u64,
    pub peak_memory: u64,
    pub memory_per_request: HashMap<String, u64>,
    pub memory_leaks_detected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorRates {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub error_rate_percentage: f64,
    pub error_types: HashMap<String, u64>,
    pub recent_errors: Vec<ErrorEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThroughputMetrics {
    pub requests_per_second: f64,
    pub average_response_time: Duration,
    pub p95_response_time: Duration,
    pub p99_response_time: Duration,
    pub concurrent_requests: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheHitRates {
    pub embeddings_cache_hit_rate: f64,
    pub responses_cache_hit_rate: f64,
    pub rag_cache_hit_rate: f64,
    pub config_cache_hit_rate: f64,
    pub total_cache_hits: u64,
    pub total_cache_misses: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorEntry {
    pub timestamp: DateTime<Utc>,
    pub error_type: String,
    pub error_message: String,
    pub request_id: Option<String>,
    pub user_id: Option<String>,
}

pub struct PerformanceMonitor {
    metrics: Arc<RwLock<PerformanceMetrics>>,
    request_timings: Arc<RwLock<HashMap<String, Vec<Duration>>>>,
    start_time: Instant,
    last_reset: Instant,
}

impl PerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(PerformanceMetrics {
                response_times: Vec::new(),
                token_usage: TokenUsage {
                    total_input_tokens: 0,
                    total_output_tokens: 0,
                    average_input_tokens: 0.0,
                    average_output_tokens: 0.0,
                    token_cost: 0.0,
                    tokens_per_request: HashMap::new(),
                },
                memory_usage: MemoryUsage {
                    current_memory: 0,
                    peak_memory: 0,
                    memory_per_request: HashMap::new(),
                    memory_leaks_detected: false,
                },
                error_rates: ErrorRates {
                    total_requests: 0,
                    successful_requests: 0,
                    failed_requests: 0,
                    error_rate_percentage: 0.0,
                    error_types: HashMap::new(),
                    recent_errors: Vec::new(),
                },
                throughput: ThroughputMetrics {
                    requests_per_second: 0.0,
                    average_response_time: Duration::from_millis(0),
                    p95_response_time: Duration::from_millis(0),
                    p99_response_time: Duration::from_millis(0),
                    concurrent_requests: 0,
                },
                cache_hit_rates: CacheHitRates {
                    embeddings_cache_hit_rate: 0.0,
                    responses_cache_hit_rate: 0.0,
                    rag_cache_hit_rate: 0.0,
                    config_cache_hit_rate: 0.0,
                    total_cache_hits: 0,
                    total_cache_misses: 0,
                },
            })),
            request_timings: Arc::new(RwLock::new(HashMap::new())),
            start_time: Instant::now(),
            last_reset: Instant::now(),
        }
    }

    pub async fn start_request(&self, request_id: &str) -> RequestTimer {
        RequestTimer::new(request_id.to_string(), self.metrics.clone())
    }

    pub async fn record_response_time(&self, duration: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.response_times.push(duration);
        
        // Keep only last 1000 response times to prevent memory bloat
        if metrics.response_times.len() > 1000 {
            metrics.response_times.remove(0);
        }
        
        self.update_throughput_metrics(&mut metrics).await;
    }

    pub async fn record_token_usage(&self, input_tokens: u64, output_tokens: u64, cost: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.token_usage.total_input_tokens += input_tokens;
        metrics.token_usage.total_output_tokens += output_tokens;
        metrics.token_usage.token_cost += cost;
        
        let total_requests = metrics.error_rates.total_requests as f64;
        metrics.token_usage.average_input_tokens = metrics.token_usage.total_input_tokens as f64 / total_requests;
        metrics.token_usage.average_output_tokens = metrics.token_usage.total_output_tokens as f64 / total_requests;
    }

    pub async fn record_memory_usage(&self, memory_bytes: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.memory_usage.current_memory = memory_bytes;
        
        if memory_bytes > metrics.memory_usage.peak_memory {
            metrics.memory_usage.peak_memory = memory_bytes;
        }
    }

    pub async fn record_error(&self, error_type: &str, error_message: &str, request_id: Option<String>) {
        let mut metrics = self.metrics.write().await;
        metrics.error_rates.failed_requests += 1;
        metrics.error_rates.total_requests += 1;
        
        *metrics.error_rates.error_types.entry(error_type.to_string()).or_insert(0) += 1;
        
        let error_entry = ErrorEntry {
            timestamp: Utc::now(),
            error_type: error_type.to_string(),
            error_message: error_message.to_string(),
            request_id,
            user_id: None,
        };
        
        metrics.error_rates.recent_errors.push(error_entry);
        
        // Keep only last 100 errors
        if metrics.error_rates.recent_errors.len() > 100 {
            metrics.error_rates.recent_errors.remove(0);
        }
        
        self.update_error_rate(&mut metrics);
    }

    pub async fn record_success(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.error_rates.successful_requests += 1;
        metrics.error_rates.total_requests += 1;
        self.update_error_rate(&mut metrics);
    }

    pub async fn record_cache_hit(&self, cache_type: &str) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_hit_rates.total_cache_hits += 1;
        
        match cache_type {
            "embeddings" => metrics.cache_hit_rates.embeddings_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "responses" => metrics.cache_hit_rates.responses_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "rag" => metrics.cache_hit_rates.rag_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "config" => metrics.cache_hit_rates.config_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            _ => {}
        }
    }

    pub async fn record_cache_miss(&self, cache_type: &str) {
        let mut metrics = self.metrics.write().await;
        metrics.cache_hit_rates.total_cache_misses += 1;
        
        match cache_type {
            "embeddings" => metrics.cache_hit_rates.embeddings_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "responses" => metrics.cache_hit_rates.responses_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "rag" => metrics.cache_hit_rates.rag_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            "config" => metrics.cache_hit_rates.config_cache_hit_rate = 
                self.calculate_cache_hit_rate(metrics.cache_hit_rates.total_cache_hits, metrics.cache_hit_rates.total_cache_misses),
            _ => {}
        }
    }

    fn calculate_cache_hit_rate(&self, hits: u64, misses: u64) -> f64 {
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }

    async fn update_throughput_metrics(&self, metrics: &mut PerformanceMetrics) {
        if metrics.response_times.is_empty() {
            return;
        }
        
        let mut sorted_times = metrics.response_times.clone();
        sorted_times.sort();
        
        let total_requests = sorted_times.len();
        let total_time: Duration = sorted_times.iter().sum();
        
        metrics.throughput.average_response_time = total_time / total_requests as u32;
        
        // Calculate percentiles
        let p95_index = (total_requests as f64 * 0.95) as usize;
        let p99_index = (total_requests as f64 * 0.99) as usize;
        
        if p95_index < sorted_times.len() {
            metrics.throughput.p95_response_time = sorted_times[p95_index];
        }
        if p99_index < sorted_times.len() {
            metrics.throughput.p99_response_time = sorted_times[p99_index];
        }
        
        // Calculate requests per second
        let elapsed = self.start_time.elapsed();
        if elapsed.as_secs() > 0 {
            metrics.throughput.requests_per_second = total_requests as f64 / elapsed.as_secs() as f64;
        }
    }

    fn update_error_rate(&self, metrics: &mut PerformanceMetrics) {
        if metrics.error_rates.total_requests > 0 {
            metrics.error_rates.error_rate_percentage = 
                (metrics.error_rates.failed_requests as f64 / metrics.error_rates.total_requests as f64) * 100.0;
        }
    }

    pub async fn get_metrics(&self) -> PerformanceMetrics {
        self.metrics.read().await.clone()
    }

    pub async fn reset_metrics(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = PerformanceMetrics {
            response_times: Vec::new(),
            token_usage: TokenUsage {
                total_input_tokens: 0,
                total_output_tokens: 0,
                average_input_tokens: 0.0,
                average_output_tokens: 0.0,
                token_cost: 0.0,
                tokens_per_request: HashMap::new(),
            },
            memory_usage: MemoryUsage {
                current_memory: 0,
                peak_memory: 0,
                memory_per_request: HashMap::new(),
                memory_leaks_detected: false,
            },
            error_rates: ErrorRates {
                total_requests: 0,
                successful_requests: 0,
                failed_requests: 0,
                error_rate_percentage: 0.0,
                error_types: HashMap::new(),
                recent_errors: Vec::new(),
            },
            throughput: ThroughputMetrics {
                requests_per_second: 0.0,
                average_response_time: Duration::from_millis(0),
                p95_response_time: Duration::from_millis(0),
                p99_response_time: Duration::from_millis(0),
                concurrent_requests: 0,
            },
            cache_hit_rates: CacheHitRates {
                embeddings_cache_hit_rate: 0.0,
                responses_cache_hit_rate: 0.0,
                rag_cache_hit_rate: 0.0,
                config_cache_hit_rate: 0.0,
                total_cache_hits: 0,
                total_cache_misses: 0,
            },
        };
    }

    pub async fn generate_report(&self) -> PerformanceReport {
        let metrics = self.get_metrics().await;
        let uptime = self.start_time.elapsed();
        
        PerformanceReport {
            timestamp: Utc::now(),
            uptime,
            metrics,
            recommendations: self.generate_recommendations(&metrics).await,
        }
    }

    async fn generate_recommendations(&self, metrics: &PerformanceMetrics) -> Vec<String> {
        let mut recommendations = Vec::new();
        
        // Response time recommendations
        if metrics.throughput.average_response_time > Duration::from_secs(5) {
            recommendations.push("Consider optimizing response times - average is over 5 seconds".to_string());
        }
        
        if metrics.throughput.p95_response_time > Duration::from_secs(10) {
            recommendations.push("95th percentile response time is high - consider caching or optimization".to_string());
        }
        
        // Error rate recommendations
        if metrics.error_rates.error_rate_percentage > 5.0 {
            recommendations.push("Error rate is high (>5%) - investigate error patterns".to_string());
        }
        
        // Cache recommendations
        if metrics.cache_hit_rates.total_cache_hits > 0 {
            let overall_hit_rate = self.calculate_cache_hit_rate(
                metrics.cache_hit_rates.total_cache_hits,
                metrics.cache_hit_rates.total_cache_misses
            );
            
            if overall_hit_rate < 0.5 {
                recommendations.push("Cache hit rate is low - consider increasing cache size or optimizing cache keys".to_string());
            }
        }
        
        // Memory recommendations
        if metrics.memory_usage.current_memory > 1_000_000_000 { // 1GB
            recommendations.push("Memory usage is high - consider memory optimization".to_string());
        }
        
        // Throughput recommendations
        if metrics.throughput.requests_per_second < 1.0 {
            recommendations.push("Throughput is low - consider performance optimization".to_string());
        }
        
        recommendations
    }
}

pub struct RequestTimer {
    request_id: String,
    start_time: Instant,
    metrics: Arc<RwLock<PerformanceMetrics>>,
}

impl RequestTimer {
    fn new(request_id: String, metrics: Arc<RwLock<PerformanceMetrics>>) -> Self {
        Self {
            request_id,
            start_time: Instant::now(),
            metrics,
        }
    }

    pub async fn finish(self) -> Duration {
        let duration = self.start_time.elapsed();
        self.metrics.read().await.record_response_time(duration).await;
        duration
    }

    pub async fn finish_with_success(self) -> Duration {
        let duration = self.finish().await;
        self.metrics.read().await.record_success().await;
        duration
    }

    pub async fn finish_with_error(self, error_type: &str, error_message: &str) -> Duration {
        let duration = self.finish().await;
        self.metrics.read().await.record_error(error_type, error_message, Some(self.request_id)).await;
        duration
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub timestamp: DateTime<Utc>,
    pub uptime: Duration,
    pub metrics: PerformanceMetrics,
    pub recommendations: Vec<String>,
}

impl PerformanceReport {
    pub fn to_markdown(&self) -> String {
        let mut report = String::new();
        
        report.push_str(&format!("# Performance Report\n\n"));
        report.push_str(&format!("**Generated:** {}\n", self.timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
        report.push_str(&format!("**Uptime:** {:.2} hours\n\n", self.uptime.as_secs_f64() / 3600.0));
        
        // Throughput
        report.push_str("## Throughput\n");
        report.push_str(&format!("- Requests per second: {:.2}\n", self.metrics.throughput.requests_per_second));
        report.push_str(&format!("- Average response time: {:.2?}\n", self.metrics.throughput.average_response_time));
        report.push_str(&format!("- 95th percentile: {:.2?}\n", self.metrics.throughput.p95_response_time));
        report.push_str(&format!("- 99th percentile: {:.2?}\n\n", self.metrics.throughput.p99_response_time));
        
        // Token usage
        report.push_str("## Token Usage\n");
        report.push_str(&format!("- Total input tokens: {}\n", self.metrics.token_usage.total_input_tokens));
        report.push_str(&format!("- Total output tokens: {}\n", self.metrics.token_usage.total_output_tokens));
        report.push_str(&format!("- Average input tokens: {:.2}\n", self.metrics.token_usage.average_input_tokens));
        report.push_str(&format!("- Average output tokens: {:.2}\n", self.metrics.token_usage.average_output_tokens));
        report.push_str(&format!("- Total cost: ${:.4}\n\n", self.metrics.token_usage.token_cost));
        
        // Error rates
        report.push_str("## Error Rates\n");
        report.push_str(&format!("- Total requests: {}\n", self.metrics.error_rates.total_requests));
        report.push_str(&format!("- Successful requests: {}\n", self.metrics.error_rates.successful_requests));
        report.push_str(&format!("- Failed requests: {}\n", self.metrics.error_rates.failed_requests));
        report.push_str(&format!("- Error rate: {:.2}%\n\n", self.metrics.error_rates.error_rate_percentage));
        
        // Cache hit rates
        report.push_str("## Cache Performance\n");
        report.push_str(&format!("- Embeddings cache hit rate: {:.2}%\n", self.metrics.cache_hit_rates.embeddings_cache_hit_rate * 100.0));
        report.push_str(&format!("- Responses cache hit rate: {:.2}%\n", self.metrics.cache_hit_rates.responses_cache_hit_rate * 100.0));
        report.push_str(&format!("- RAG cache hit rate: {:.2}%\n", self.metrics.cache_hit_rates.rag_cache_hit_rate * 100.0));
        report.push_str(&format!("- Config cache hit rate: {:.2}%\n\n", self.metrics.cache_hit_rates.config_cache_hit_rate * 100.0));
        
        // Memory usage
        report.push_str("## Memory Usage\n");
        report.push_str(&format!("- Current memory: {:.2} MB\n", self.metrics.memory_usage.current_memory as f64 / 1_048_576.0));
        report.push_str(&format!("- Peak memory: {:.2} MB\n\n", self.metrics.memory_usage.peak_memory as f64 / 1_048_576.0));
        
        // Recommendations
        if !self.recommendations.is_empty() {
            report.push_str("## Recommendations\n");
            for recommendation in &self.recommendations {
                report.push_str(&format!("- {}\n", recommendation));
            }
        }
        
        report
    }
}

// Benchmarking utilities
pub struct BenchmarkRunner {
    monitor: PerformanceMonitor,
    test_scenarios: Vec<BenchmarkScenario>,
}

#[derive(Debug, Clone)]
pub struct BenchmarkScenario {
    pub name: String,
    pub description: String,
    pub requests: Vec<BenchmarkRequest>,
    pub concurrent_users: u32,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct BenchmarkRequest {
    pub name: String,
    pub prompt: String,
    pub expected_tokens: Option<u64>,
    pub timeout: Duration,
}

impl BenchmarkRunner {
    pub fn new() -> Self {
        Self {
            monitor: PerformanceMonitor::new(),
            test_scenarios: Vec::new(),
        }
    }

    pub fn add_scenario(&mut self, scenario: BenchmarkScenario) {
        self.test_scenarios.push(scenario);
    }

    pub async fn run_benchmarks(&self) -> Vec<BenchmarkResult> {
        let mut results = Vec::new();
        
        for scenario in &self.test_scenarios {
            let result = self.run_scenario(scenario).await;
            results.push(result);
        }
        
        results
    }

    async fn run_scenario(&self, scenario: &BenchmarkScenario) -> BenchmarkResult {
        let start_time = Instant::now();
        let mut successful_requests = 0;
        let mut failed_requests = 0;
        let mut total_tokens = 0;
        let mut response_times = Vec::new();
        
        // Run the benchmark
        while start_time.elapsed() < scenario.duration {
            for request in &scenario.requests {
                let timer = self.monitor.start_request(&request.name).await;
                
                // Simulate request processing
                let response_time = Duration::from_millis(rand::random::<u64>() % 5000 + 100);
                response_times.push(response_time);
                
                if rand::random::<f64>() > 0.1 { // 90% success rate
                    successful_requests += 1;
                    timer.finish_with_success().await;
                } else {
                    failed_requests += 1;
                    timer.finish_with_error("timeout", "Request timed out").await;
                }
                
                total_tokens += request.expected_tokens.unwrap_or(100);
            }
        }
        
        BenchmarkResult {
            scenario_name: scenario.name.clone(),
            duration: scenario.duration,
            successful_requests,
            failed_requests,
            total_tokens,
            average_response_time: response_times.iter().sum::<Duration>() / response_times.len() as u32,
            throughput: successful_requests as f64 / scenario.duration.as_secs_f64(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    pub scenario_name: String,
    pub duration: Duration,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_tokens: u64,
    pub average_response_time: Duration,
    pub throughput: f64,
}