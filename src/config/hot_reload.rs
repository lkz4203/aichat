use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::time::sleep;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotReloadConfig {
    pub enabled: bool,
    pub watch_paths: Vec<PathBuf>,
    pub debounce_ms: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
}

impl Default for HotReloadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            watch_paths: vec![
                PathBuf::from("config.yaml"),
                PathBuf::from("roles/"),
                PathBuf::from("macros/"),
                PathBuf::from("rags/"),
            ],
            debounce_ms: 500,
            max_retries: 3,
            retry_delay_ms: 1000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConfigChangeEvent {
    pub path: PathBuf,
    pub event_type: ConfigChangeType,
    pub timestamp: std::time::Instant,
}

#[derive(Debug, Clone)]
pub enum ConfigChangeType {
    Created,
    Modified,
    Removed,
    Renamed,
}

pub struct HotReloadManager {
    config: HotReloadConfig,
    watcher: Option<notify::RecommendedWatcher>,
    change_tx: mpsc::Sender<ConfigChangeEvent>,
    change_rx: mpsc::Receiver<ConfigChangeEvent>,
    callbacks: Arc<RwLock<HashMap<String, Box<dyn Fn(ConfigChangeEvent) + Send + Sync>>>>,
    is_watching: Arc<RwLock<bool>>,
}

impl HotReloadManager {
    pub fn new(config: HotReloadConfig) -> Self {
        let (change_tx, change_rx) = mpsc::channel(100);
        Self {
            config,
            watcher: None,
            change_tx,
            change_rx,
            callbacks: Arc::new(RwLock::new(HashMap::new())),
            is_watching: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn start_watching(&mut self) -> Result<()> {
        if *self.is_watching.read() {
            return Ok(());
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        })?;

        // Watch all configured paths
        for path in &self.config.watch_paths {
            if path.exists() {
                if path.is_dir() {
                    watcher.watch(path, RecursiveMode::Recursive)?;
                } else {
                    if let Some(parent) = path.parent() {
                        watcher.watch(parent, RecursiveMode::NonRecursive)?;
                    }
                }
            }
        }

        self.watcher = Some(watcher);
        *self.is_watching.write() = true;

        // Start the event processing loop
        let change_tx = self.change_tx.clone();
        let callbacks = self.callbacks.clone();
        let debounce_ms = self.config.debounce_ms;

        tokio::spawn(async move {
            let mut pending_events: HashMap<PathBuf, ConfigChangeEvent> = HashMap::new();
            
            while let Ok(event) = rx.recv() {
                for path in event.paths {
                    let change_type = match event.kind {
                        EventKind::Create(_) => ConfigChangeType::Created,
                        EventKind::Modify(_) => ConfigChangeType::Modified,
                        EventKind::Remove(_) => ConfigChangeType::Removed,
                        EventKind::Rename(_) => ConfigChangeType::Renamed,
                        _ => continue,
                    };

                    let change_event = ConfigChangeEvent {
                        path,
                        event_type: change_type,
                        timestamp: std::time::Instant::now(),
                    };

                    pending_events.insert(change_event.path.clone(), change_event);
                }

                // Debounce events
                sleep(Duration::from_millis(debounce_ms)).await;

                // Process pending events
                for (_, event) in pending_events.drain() {
                    let _ = change_tx.send(event).await;
                }
            }
        });

        // Start the callback processing loop
        let callbacks = self.callbacks.clone();
        tokio::spawn(async move {
            while let Some(event) = self.change_rx.recv().await {
                let callbacks = callbacks.read();
                for callback in callbacks.values() {
                    callback(event.clone());
                }
            }
        });

        Ok(())
    }

    pub fn stop_watching(&mut self) -> Result<()> {
        if let Some(watcher) = self.watcher.take() {
            watcher.unwatch(Path::new("."))?;
        }
        *self.is_watching.write() = false;
        Ok(())
    }

    pub fn register_callback<F>(&self, name: String, callback: F) -> Result<()>
    where
        F: Fn(ConfigChangeEvent) + Send + Sync + 'static,
    {
        let mut callbacks = self.callbacks.write();
        callbacks.insert(name, Box::new(callback));
        Ok(())
    }

    pub fn unregister_callback(&self, name: &str) -> Result<()> {
        let mut callbacks = self.callbacks.write();
        callbacks.remove(name);
        Ok(())
    }

    pub fn is_watching(&self) -> bool {
        *self.is_watching.read()
    }

    pub async fn reload_config(&self, config_path: &Path) -> Result<()> {
        let mut retries = 0;
        while retries < self.config.max_retries {
            match self.try_reload_config(config_path).await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    retries += 1;
                    if retries < self.config.max_retries {
                        log::warn!("Failed to reload config (attempt {}/{}): {}", 
                                  retries, self.config.max_retries, e);
                        sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    async fn try_reload_config(&self, config_path: &Path) -> Result<()> {
        // Wait a bit to ensure file is fully written
        sleep(Duration::from_millis(100)).await;
        
        // Check if file exists and is readable
        if !config_path.exists() {
            return Err(anyhow::anyhow!("Config file does not exist: {:?}", config_path));
        }

        // Try to read and parse the config
        let content = std::fs::read_to_string(config_path)?;
        let _: serde_yaml::Value = serde_yaml::from_str(&content)?;
        
        log::info!("Configuration reloaded successfully from {:?}", config_path);
        Ok(())
    }
}

// Environment-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentConfig {
    pub environment: String,
    pub config_overrides: HashMap<String, serde_json::Value>,
    pub feature_flags: HashMap<String, bool>,
}

impl EnvironmentConfig {
    pub fn new(environment: String) -> Self {
        Self {
            environment,
            config_overrides: HashMap::new(),
            feature_flags: HashMap::new(),
        }
    }

    pub fn with_override(mut self, key: String, value: serde_json::Value) -> Self {
        self.config_overrides.insert(key, value);
        self
    }

    pub fn with_feature_flag(mut self, flag: String, enabled: bool) -> Self {
        self.feature_flags.insert(flag, enabled);
        self
    }

    pub fn is_feature_enabled(&self, flag: &str) -> bool {
        self.feature_flags.get(flag).copied().unwrap_or(false)
    }

    pub fn get_override(&self, key: &str) -> Option<&serde_json::Value> {
        self.config_overrides.get(key)
    }
}

// Configuration validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidationRule {
    pub field: String,
    pub required: bool,
    pub validator: Option<String>, // JSON Schema validation
    pub default_value: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigSchema {
    pub version: String,
    pub rules: Vec<ConfigValidationRule>,
    pub additional_properties: bool,
}

impl ConfigSchema {
    pub fn validate(&self, config: &serde_json::Value) -> Result<Vec<String>> {
        let mut errors = Vec::new();

        for rule in &self.rules {
            if let Some(value) = config.get(&rule.field) {
                // Validate against JSON Schema if provided
                if let Some(schema_str) = &rule.validator {
                    if let Err(e) = self.validate_json_schema(value, schema_str) {
                        errors.push(format!("Field '{}': {}", rule.field, e));
                    }
                }
            } else if rule.required {
                errors.push(format!("Required field '{}' is missing", rule.field));
            }
        }

        Ok(errors)
    }

    fn validate_json_schema(&self, value: &serde_json::Value, schema: &str) -> Result<()> {
        // This is a simplified validation - in a real implementation,
        // you would use a proper JSON Schema validation library
        let schema_value: serde_json::Value = serde_json::from_str(schema)?;
        
        // Basic type checking
        match (value, &schema_value) {
            (serde_json::Value::String(_), serde_json::Value::Object(schema_obj)) => {
                if let Some(serde_json::Value::String(schema_type)) = schema_obj.get("type") {
                    if schema_type != "string" {
                        return Err(anyhow::anyhow!("Expected string, got different type"));
                    }
                }
            }
            (serde_json::Value::Number(_), serde_json::Value::Object(schema_obj)) => {
                if let Some(serde_json::Value::String(schema_type)) = schema_obj.get("type") {
                    if schema_type != "number" && schema_type != "integer" {
                        return Err(anyhow::anyhow!("Expected number, got different type"));
                    }
                }
            }
            (serde_json::Value::Bool(_), serde_json::Value::Object(schema_obj)) => {
                if let Some(serde_json::Value::String(schema_type)) = schema_obj.get("type") {
                    if schema_type != "boolean" {
                        return Err(anyhow::anyhow!("Expected boolean, got different type"));
                    }
                }
            }
            _ => {}
        }

        Ok(())
    }
}

pub struct ConfigValidator {
    schema: ConfigSchema,
}

impl ConfigValidator {
    pub fn new(schema: ConfigSchema) -> Self {
        Self { schema }
    }

    pub fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let errors = self.schema.validate(config)?;
        if !errors.is_empty() {
            return Err(anyhow::anyhow!("Configuration validation failed:\n{}", 
                                      errors.join("\n")));
        }
        Ok(())
    }
}