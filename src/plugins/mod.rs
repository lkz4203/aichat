use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use async_trait::async_trait;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub commands: Vec<PluginCommand>,
    pub hooks: Vec<PluginHook>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommand {
    pub name: String,
    pub description: String,
    pub usage: String,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHook {
    pub name: String,
    pub description: String,
    pub event_type: HookEventType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HookEventType {
    PreChatCompletion,
    PostChatCompletion,
    PreRagSearch,
    PostRagSearch,
    PreFunctionCall,
    PostFunctionCall,
    ConfigLoaded,
    SessionSaved,
    SessionLoaded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    pub config: Arc<RwLock<serde_json::Value>>,
    pub session: Arc<RwLock<serde_json::Value>>,
    pub variables: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginResult {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
    pub modified_context: Option<PluginContext>,
}

#[async_trait]
pub trait AIChatPlugin: Send + Sync {
    fn info(&self) -> PluginInfo;
    
    async fn initialize(&self, context: &PluginContext) -> Result<PluginResult>;
    
    async fn execute_command(
        &self,
        command: &str,
        args: &[String],
        context: &PluginContext,
    ) -> Result<PluginResult>;
    
    async fn handle_hook(
        &self,
        hook: &PluginHook,
        context: &PluginContext,
        data: Option<serde_json::Value>,
    ) -> Result<PluginResult>;
    
    async fn cleanup(&self) -> Result<()>;
}

pub struct PluginManager {
    plugins: Arc<RwLock<HashMap<String, Box<dyn AIChatPlugin>>>>,
    context: Arc<RwLock<PluginContext>>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            context: Arc::new(RwLock::new(PluginContext {
                config: Arc::new(RwLock::new(serde_json::Value::Null)),
                session: Arc::new(RwLock::new(serde_json::Value::Null)),
                variables: HashMap::new(),
            })),
        }
    }

    pub async fn register_plugin(&self, plugin: Box<dyn AIChatPlugin>) -> Result<()> {
        let info = plugin.info();
        let name = info.name.clone();
        
        // Initialize the plugin
        let context = self.context.read().await.clone();
        plugin.initialize(&context).await?;
        
        // Register the plugin
        let mut plugins = self.plugins.write().await;
        plugins.insert(name, plugin);
        
        Ok(())
    }

    pub async fn unregister_plugin(&self, name: &str) -> Result<()> {
        let mut plugins = self.plugins.write().await;
        if let Some(plugin) = plugins.remove(name) {
            plugin.cleanup().await?;
        }
        Ok(())
    }

    pub async fn execute_command(
        &self,
        plugin_name: &str,
        command: &str,
        args: &[String],
    ) -> Result<PluginResult> {
        let plugins = self.plugins.read().await;
        if let Some(plugin) = plugins.get(plugin_name) {
            let context = self.context.read().await.clone();
            plugin.execute_command(command, args, &context).await
        } else {
            Err(anyhow::anyhow!("Plugin '{}' not found", plugin_name))
        }
    }

    pub async fn trigger_hook(
        &self,
        event_type: HookEventType,
        data: Option<serde_json::Value>,
    ) -> Result<Vec<PluginResult>> {
        let plugins = self.plugins.read().await;
        let context = self.context.read().await.clone();
        let mut results = Vec::new();

        for plugin in plugins.values() {
            let info = plugin.info();
            for hook in &info.hooks {
                if hook.event_type == event_type {
                    match plugin.handle_hook(hook, &context, data.clone()).await {
                        Ok(result) => results.push(result),
                        Err(e) => {
                            results.push(PluginResult {
                                success: false,
                                message: format!("Hook execution failed: {}", e),
                                data: None,
                                modified_context: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(results)
    }

    pub async fn list_plugins(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins.values().map(|p| p.info()).collect()
    }

    pub async fn update_context(&self, context: PluginContext) {
        *self.context.write().await = context;
    }

    pub async fn get_context(&self) -> PluginContext {
        self.context.read().await.clone()
    }
}

// Built-in plugins
pub mod builtin {
    use super::*;
    use std::collections::HashMap;

    pub struct FileManagerPlugin;

    #[async_trait]
    impl AIChatPlugin for FileManagerPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "file_manager".to_string(),
                version: "1.0.0".to_string(),
                description: "File management operations".to_string(),
                author: "AIChat Team".to_string(),
                commands: vec![
                    PluginCommand {
                        name: "list".to_string(),
                        description: "List files in directory".to_string(),
                        usage: "list <path>".to_string(),
                        examples: vec!["list .".to_string(), "list /tmp".to_string()],
                    },
                    PluginCommand {
                        name: "read".to_string(),
                        description: "Read file contents".to_string(),
                        usage: "read <file_path>".to_string(),
                        examples: vec!["read config.yaml".to_string()],
                    },
                ],
                hooks: vec![
                    PluginHook {
                        name: "pre_chat".to_string(),
                        description: "Pre-process chat input".to_string(),
                        event_type: HookEventType::PreChatCompletion,
                    },
                ],
            }
        }

        async fn initialize(&self, _context: &PluginContext) -> Result<PluginResult> {
            Ok(PluginResult {
                success: true,
                message: "FileManager plugin initialized".to_string(),
                data: None,
                modified_context: None,
            })
        }

        async fn execute_command(
            &self,
            command: &str,
            args: &[String],
            _context: &PluginContext,
        ) -> Result<PluginResult> {
            match command {
                "list" => {
                    if args.is_empty() {
                        return Err(anyhow::anyhow!("Path argument required"));
                    }
                    
                    let path = &args[0];
                    match std::fs::read_dir(path) {
                        Ok(entries) => {
                            let files: Vec<String> = entries
                                .filter_map(|entry| entry.ok())
                                .map(|entry| entry.file_name().to_string_lossy().to_string())
                                .collect();
                            
                            Ok(PluginResult {
                                success: true,
                                message: format!("Found {} files", files.len()),
                                data: Some(serde_json::json!({ "files": files })),
                                modified_context: None,
                            })
                        }
                        Err(e) => Ok(PluginResult {
                            success: false,
                            message: format!("Failed to list directory: {}", e),
                            data: None,
                            modified_context: None,
                        }),
                    }
                }
                "read" => {
                    if args.is_empty() {
                        return Err(anyhow::anyhow!("File path argument required"));
                    }
                    
                    let file_path = &args[0];
                    match std::fs::read_to_string(file_path) {
                        Ok(content) => Ok(PluginResult {
                            success: true,
                            message: format!("Read {} bytes", content.len()),
                            data: Some(serde_json::json!({ "content": content })),
                            modified_context: None,
                        }),
                        Err(e) => Ok(PluginResult {
                            success: false,
                            message: format!("Failed to read file: {}", e),
                            data: None,
                            modified_context: None,
                        }),
                    }
                }
                _ => Err(anyhow::anyhow!("Unknown command: {}", command)),
            }
        }

        async fn handle_hook(
            &self,
            _hook: &PluginHook,
            _context: &PluginContext,
            _data: Option<serde_json::Value>,
        ) -> Result<PluginResult> {
            Ok(PluginResult {
                success: true,
                message: "Hook handled".to_string(),
                data: None,
                modified_context: None,
            })
        }

        async fn cleanup(&self) -> Result<()> {
            Ok(())
        }
    }

    pub struct NetworkPlugin;

    #[async_trait]
    impl AIChatPlugin for NetworkPlugin {
        fn info(&self) -> PluginInfo {
            PluginInfo {
                name: "network".to_string(),
                version: "1.0.0".to_string(),
                description: "Network operations".to_string(),
                author: "AIChat Team".to_string(),
                commands: vec![
                    PluginCommand {
                        name: "fetch".to_string(),
                        description: "Fetch URL content".to_string(),
                        usage: "fetch <url>".to_string(),
                        examples: vec!["fetch https://api.github.com".to_string()],
                    },
                ],
                hooks: vec![],
            }
        }

        async fn initialize(&self, _context: &PluginContext) -> Result<PluginResult> {
            Ok(PluginResult {
                success: true,
                message: "Network plugin initialized".to_string(),
                data: None,
                modified_context: None,
            })
        }

        async fn execute_command(
            &self,
            command: &str,
            args: &[String],
            _context: &PluginContext,
        ) -> Result<PluginResult> {
            match command {
                "fetch" => {
                    if args.is_empty() {
                        return Err(anyhow::anyhow!("URL argument required"));
                    }
                    
                    let url = &args[0];
                    match reqwest::get(url).await {
                        Ok(response) => {
                            match response.text().await {
                                Ok(content) => Ok(PluginResult {
                                    success: true,
                                    message: format!("Fetched {} bytes", content.len()),
                                    data: Some(serde_json::json!({ "content": content })),
                                    modified_context: None,
                                }),
                                Err(e) => Ok(PluginResult {
                                    success: false,
                                    message: format!("Failed to read response: {}", e),
                                    data: None,
                                    modified_context: None,
                                }),
                            }
                        }
                        Err(e) => Ok(PluginResult {
                            success: false,
                            message: format!("Failed to fetch URL: {}", e),
                            data: None,
                            modified_context: None,
                        }),
                    }
                }
                _ => Err(anyhow::anyhow!("Unknown command: {}", command)),
            }
        }

        async fn handle_hook(
            &self,
            _hook: &PluginHook,
            _context: &PluginContext,
            _data: Option<serde_json::Value>,
        ) -> Result<PluginResult> {
            Ok(PluginResult {
                success: true,
                message: "Hook handled".to_string(),
                data: None,
                modified_context: None,
            })
        }

        async fn cleanup(&self) -> Result<()> {
            Ok(())
        }
    }
}