use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use fuzzy_matcher::{FuzzyMatcher, SkimMatcherV2};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestionContext {
    pub current_command: Option<String>,
    pub current_role: Option<String>,
    pub current_session: Option<String>,
    pub current_rag: Option<String>,
    pub recent_commands: Vec<String>,
    pub user_preferences: HashMap<String, String>,
    pub system_info: SystemInfo,
    pub conversation_history: Vec<ConversationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub os: String,
    pub shell: String,
    pub current_dir: String,
    pub git_branch: Option<String>,
    pub available_memory: u64,
    pub cpu_cores: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_input: String,
    pub ai_response: String,
    pub tokens_used: Option<usize>,
    pub model_used: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartSuggestion {
    pub command: String,
    pub description: String,
    pub confidence: f64,
    pub category: SuggestionCategory,
    pub examples: Vec<String>,
    pub context_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SuggestionCategory {
    Command,
    Role,
    Model,
    Session,
    Rag,
    Macro,
    Function,
    Tool,
}

pub struct SmartSuggestionEngine {
    context: Arc<RwLock<SuggestionContext>>,
    command_patterns: HashMap<String, CommandPattern>,
    usage_statistics: Arc<RwLock<HashMap<String, u64>>>,
    matcher: SkimMatcherV2,
}

impl SmartSuggestionEngine {
    pub fn new() -> Self {
        Self {
            context: Arc::new(RwLock::new(SuggestionContext {
                current_command: None,
                current_role: None,
                current_session: None,
                current_rag: None,
                recent_commands: Vec::new(),
                user_preferences: HashMap::new(),
                system_info: SystemInfo {
                    os: std::env::consts::OS.to_string(),
                    shell: std::env::var("SHELL").unwrap_or_else(|_| "unknown".to_string()),
                    current_dir: std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."))
                        .to_string_lossy()
                        .to_string(),
                    git_branch: None,
                    available_memory: 0,
                    cpu_cores: num_cpus::get() as u32,
                },
                conversation_history: Vec::new(),
            })),
            command_patterns: Self::initialize_command_patterns(),
            usage_statistics: Arc::new(RwLock::new(HashMap::new())),
            matcher: SkimMatcherV2::default(),
        }
    }

    fn initialize_command_patterns() -> HashMap<String, CommandPattern> {
        let mut patterns = HashMap::new();
        
        // Basic commands
        patterns.insert("help".to_string(), CommandPattern {
            description: "Show help information".to_string(),
            usage: "help [command]".to_string(),
            examples: vec!["help".to_string(), "help session".to_string()],
            category: SuggestionCategory::Command,
            context_hints: vec!["new user".to_string(), "confused".to_string()],
        });

        patterns.insert("session".to_string(), CommandPattern {
            description: "Session management".to_string(),
            usage: "session [name]".to_string(),
            examples: vec!["session".to_string(), "session my_project".to_string()],
            category: SuggestionCategory::Session,
            context_hints: vec!["long conversation".to_string(), "multiple topics".to_string()],
        });

        patterns.insert("role".to_string(), CommandPattern {
            description: "Role management".to_string(),
            usage: "role [name]".to_string(),
            examples: vec!["role".to_string(), "role programmer".to_string()],
            category: SuggestionCategory::Role,
            context_hints: vec!["specific task".to_string(), "expertise needed".to_string()],
        });

        patterns.insert("rag".to_string(), CommandPattern {
            description: "RAG operations".to_string(),
            usage: "rag [name]".to_string(),
            examples: vec!["rag".to_string(), "rag docs".to_string()],
            category: SuggestionCategory::Rag,
            context_hints: vec!["documentation".to_string(), "knowledge base".to_string()],
        });

        patterns.insert("agent".to_string(), CommandPattern {
            description: "Agent operations".to_string(),
            usage: "agent [name]".to_string(),
            examples: vec!["agent".to_string(), "agent assistant".to_string()],
            category: SuggestionCategory::Tool,
            context_hints: vec!["automation".to_string(), "complex task".to_string()],
        });

        patterns.insert("macro".to_string(), CommandPattern {
            description: "Macro operations".to_string(),
            usage: "macro [name]".to_string(),
            examples: vec!["macro".to_string(), "macro build".to_string()],
            category: SuggestionCategory::Macro,
            context_hints: vec!["repetitive task".to_string(), "workflow".to_string()],
        });

        patterns
    }

    pub async fn get_suggestions(&self, input: &str) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();
        let context = self.context.read().await;

        // Command suggestions
        suggestions.extend(self.get_command_suggestions(input, &context).await);

        // Role suggestions
        suggestions.extend(self.get_role_suggestions(input, &context).await);

        // Model suggestions
        suggestions.extend(self.get_model_suggestions(input, &context).await);

        // Session suggestions
        suggestions.extend(self.get_session_suggestions(input, &context).await);

        // RAG suggestions
        suggestions.extend(self.get_rag_suggestions(input, &context).await);

        // Context-aware suggestions
        suggestions.extend(self.get_context_aware_suggestions(input, &context).await);

        // Sort by confidence and usage
        suggestions.sort_by(|a, b| {
            let confidence_cmp = b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal);
            if confidence_cmp == std::cmp::Ordering::Equal {
                let usage_a = self.get_usage_count(&a.command).await;
                let usage_b = self.get_usage_count(&b.command).await;
                usage_b.cmp(&usage_a)
            } else {
                confidence_cmp
            }
        });

        suggestions
    }

    async fn get_command_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();

        for (cmd, pattern) in &self.command_patterns {
            let confidence = self.calculate_confidence(input, cmd, pattern, context);
            if confidence > 0.1 {
                suggestions.push(SmartSuggestion {
                    command: cmd.clone(),
                    description: pattern.description.clone(),
                    confidence,
                    category: pattern.category.clone(),
                    examples: pattern.examples.clone(),
                    context_hints: pattern.context_hints.clone(),
                });
            }
        }

        suggestions
    }

    async fn get_role_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();
        let available_roles = self.get_available_roles().await;

        for role in available_roles {
            let confidence = self.calculate_role_confidence(input, &role, context);
            if confidence > 0.1 {
                suggestions.push(SmartSuggestion {
                    command: format!("role {}", role),
                    description: format!("Switch to {} role", role),
                    confidence,
                    category: SuggestionCategory::Role,
                    examples: vec![format!("role {}", role)],
                    context_hints: vec!["role switch".to_string()],
                });
            }
        }

        suggestions
    }

    async fn get_model_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();
        let available_models = self.get_available_models().await;

        for model in available_models {
            let confidence = self.calculate_model_confidence(input, &model, context);
            if confidence > 0.1 {
                suggestions.push(SmartSuggestion {
                    command: format!("model {}", model),
                    description: format!("Switch to {} model", model),
                    confidence,
                    category: SuggestionCategory::Model,
                    examples: vec![format!("model {}", model)],
                    context_hints: vec!["model switch".to_string()],
                });
            }
        }

        suggestions
    }

    async fn get_session_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();
        let available_sessions = self.get_available_sessions().await;

        for session in available_sessions {
            let confidence = self.calculate_session_confidence(input, &session, context);
            if confidence > 0.1 {
                suggestions.push(SmartSuggestion {
                    command: format!("session {}", session),
                    description: format!("Switch to {} session", session),
                    confidence,
                    category: SuggestionCategory::Session,
                    examples: vec![format!("session {}", session)],
                    context_hints: vec!["session switch".to_string()],
                });
            }
        }

        suggestions
    }

    async fn get_rag_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();
        let available_rags = self.get_available_rags().await;

        for rag in available_rags {
            let confidence = self.calculate_rag_confidence(input, &rag, context);
            if confidence > 0.1 {
                suggestions.push(SmartSuggestion {
                    command: format!("rag {}", rag),
                    description: format!("Use {} RAG", rag),
                    confidence,
                    category: SuggestionCategory::Rag,
                    examples: vec![format!("rag {}", rag)],
                    context_hints: vec!["documentation".to_string()],
                });
            }
        }

        suggestions
    }

    async fn get_context_aware_suggestions(&self, input: &str, context: &SuggestionContext) -> Vec<SmartSuggestion> {
        let mut suggestions = Vec::new();

        // Analyze conversation history for patterns
        if let Some(pattern) = self.analyze_conversation_pattern(&context.conversation_history) {
            suggestions.push(SmartSuggestion {
                command: pattern.command.clone(),
                description: pattern.description.clone(),
                confidence: 0.8,
                category: SuggestionCategory::Command,
                examples: pattern.examples.clone(),
                context_hints: vec!["conversation pattern".to_string()],
            });
        }

        // Suggest based on current working directory
        if let Some(suggestion) = self.get_directory_based_suggestion(&context.system_info.current_dir) {
            suggestions.push(suggestion);
        }

        // Suggest based on recent commands
        if let Some(suggestion) = self.get_recent_command_suggestion(&context.recent_commands) {
            suggestions.push(suggestion);
        }

        suggestions
    }

    fn calculate_confidence(&self, input: &str, command: &str, pattern: &CommandPattern, context: &SuggestionContext) -> f64 {
        let mut confidence = 0.0;

        // Exact match
        if command.starts_with(input) {
            confidence += 0.8;
        }

        // Fuzzy match
        if let Some(score) = self.matcher.fuzzy_match(command, input) {
            confidence += (score as f64) / 100.0;
        }

        // Context hints
        for hint in &pattern.context_hints {
            if self.context_matches_hint(context, hint) {
                confidence += 0.2;
            }
        }

        // Usage frequency
        let usage = self.get_usage_count_sync(command);
        confidence += (usage as f64).min(10.0) / 10.0 * 0.1;

        confidence.min(1.0)
    }

    fn calculate_role_confidence(&self, input: &str, role: &str, context: &SuggestionContext) -> f64 {
        let mut confidence = 0.0;

        if role.starts_with(input) {
            confidence += 0.7;
        }

        if let Some(score) = self.matcher.fuzzy_match(role, input) {
            confidence += (score as f64) / 100.0;
        }

        confidence.min(1.0)
    }

    fn calculate_model_confidence(&self, input: &str, model: &str, context: &SuggestionContext) -> f64 {
        let mut confidence = 0.0;

        if model.starts_with(input) {
            confidence += 0.7;
        }

        if let Some(score) = self.matcher.fuzzy_match(model, input) {
            confidence += (score as f64) / 100.0;
        }

        confidence.min(1.0)
    }

    fn calculate_session_confidence(&self, input: &str, session: &str, context: &SuggestionContext) -> f64 {
        let mut confidence = 0.0;

        if session.starts_with(input) {
            confidence += 0.7;
        }

        if let Some(score) = self.matcher.fuzzy_match(session, input) {
            confidence += (score as f64) / 100.0;
        }

        confidence.min(1.0)
    }

    fn calculate_rag_confidence(&self, input: &str, rag: &str, context: &SuggestionContext) -> f64 {
        let mut confidence = 0.0;

        if rag.starts_with(input) {
            confidence += 0.7;
        }

        if let Some(score) = self.matcher.fuzzy_match(rag, input) {
            confidence += (score as f64) / 100.0;
        }

        confidence.min(1.0)
    }

    fn context_matches_hint(&self, context: &SuggestionContext, hint: &str) -> bool {
        match hint {
            "new user" => context.recent_commands.len() < 5,
            "confused" => context.recent_commands.iter().any(|cmd| cmd.contains("help")),
            "long conversation" => context.conversation_history.len() > 10,
            "multiple topics" => context.conversation_history.len() > 5,
            "specific task" => context.current_role.is_some(),
            "expertise needed" => context.current_role.is_some(),
            "documentation" => context.current_rag.is_some(),
            "knowledge base" => context.current_rag.is_some(),
            "automation" => context.recent_commands.iter().any(|cmd| cmd.contains("agent")),
            "complex task" => context.conversation_history.len() > 3,
            "repetitive task" => self.has_repetitive_pattern(&context.recent_commands),
            "workflow" => context.recent_commands.iter().any(|cmd| cmd.contains("macro")),
            _ => false,
        }
    }

    fn has_repetitive_pattern(&self, commands: &[String]) -> bool {
        if commands.len() < 3 {
            return false;
        }

        let last_three: Vec<&str> = commands.iter().rev().take(3).map(|s| s.as_str()).collect();
        last_three.windows(2).any(|window| window[0] == window[1])
    }

    async fn get_usage_count(&self, command: &str) -> u64 {
        let stats = self.usage_statistics.read().await;
        stats.get(command).copied().unwrap_or(0)
    }

    fn get_usage_count_sync(&self, command: &str) -> u64 {
        // This is a simplified version - in a real implementation you'd need proper sync
        0
    }

    async fn get_available_roles(&self) -> Vec<String> {
        // This would typically load from the roles directory
        vec![
            "programmer".to_string(),
            "writer".to_string(),
            "analyst".to_string(),
            "teacher".to_string(),
            "assistant".to_string(),
        ]
    }

    async fn get_available_models(&self) -> Vec<String> {
        // This would typically load from the models configuration
        vec![
            "gpt-4".to_string(),
            "gpt-3.5-turbo".to_string(),
            "claude-3".to_string(),
            "gemini-pro".to_string(),
        ]
    }

    async fn get_available_sessions(&self) -> Vec<String> {
        // This would typically load from the sessions directory
        vec![
            "project_a".to_string(),
            "project_b".to_string(),
            "research".to_string(),
        ]
    }

    async fn get_available_rags(&self) -> Vec<String> {
        // This would typically load from the rags directory
        vec![
            "docs".to_string(),
            "codebase".to_string(),
            "research".to_string(),
        ]
    }

    fn analyze_conversation_pattern(&self, history: &[ConversationEntry]) -> Option<CommandPattern> {
        if history.len() < 3 {
            return None;
        }

        // Look for patterns in recent conversations
        let recent: Vec<&str> = history.iter().rev().take(5)
            .map(|entry| entry.user_input.as_str())
            .collect();

        // Check for help-seeking patterns
        if recent.iter().any(|input| input.contains("help") || input.contains("?")) {
            return Some(CommandPattern {
                description: "Get help with available commands".to_string(),
                usage: "help".to_string(),
                examples: vec!["help".to_string()],
                category: SuggestionCategory::Command,
                context_hints: vec!["confused".to_string()],
            });
        }

        // Check for session management patterns
        if recent.iter().any(|input| input.contains("session")) {
            return Some(CommandPattern {
                description: "Manage conversation sessions".to_string(),
                usage: "session [name]".to_string(),
                examples: vec!["session".to_string(), "session new".to_string()],
                category: SuggestionCategory::Session,
                context_hints: vec!["session management".to_string()],
            });
        }

        None
    }

    fn get_directory_based_suggestion(&self, current_dir: &str) -> Option<SmartSuggestion> {
        if current_dir.contains("src") || current_dir.contains("code") {
            return Some(SmartSuggestion {
                command: "role programmer".to_string(),
                description: "Switch to programmer role for code-related tasks".to_string(),
                confidence: 0.7,
                category: SuggestionCategory::Role,
                examples: vec!["role programmer".to_string()],
                context_hints: vec!["code directory".to_string()],
            });
        }

        if current_dir.contains("docs") || current_dir.contains("documentation") {
            return Some(SmartSuggestion {
                command: "rag docs".to_string(),
                description: "Use documentation RAG for better context".to_string(),
                confidence: 0.7,
                category: SuggestionCategory::Rag,
                examples: vec!["rag docs".to_string()],
                context_hints: vec!["documentation directory".to_string()],
            });
        }

        None
    }

    fn get_recent_command_suggestion(&self, recent_commands: &[String]) -> Option<SmartSuggestion> {
        if recent_commands.is_empty() {
            return None;
        }

        let last_command = &recent_commands[recent_commands.len() - 1];
        
        // Suggest related commands based on the last command
        match last_command.as_str() {
            cmd if cmd.contains("session") => Some(SmartSuggestion {
                command: "session list".to_string(),
                description: "List all available sessions".to_string(),
                confidence: 0.6,
                category: SuggestionCategory::Session,
                examples: vec!["session list".to_string()],
                context_hints: vec!["session management".to_string()],
            }),
            cmd if cmd.contains("role") => Some(SmartSuggestion {
                command: "role list".to_string(),
                description: "List all available roles".to_string(),
                confidence: 0.6,
                category: SuggestionCategory::Role,
                examples: vec!["role list".to_string()],
                context_hints: vec!["role management".to_string()],
            }),
            _ => None,
        }
    }

    pub async fn update_context(&self, new_context: SuggestionContext) {
        *self.context.write().await = new_context;
    }

    pub async fn record_command_usage(&self, command: String) {
        let mut stats = self.usage_statistics.write().await;
        *stats.entry(command).or_insert(0) += 1;
    }
}

#[derive(Debug, Clone)]
pub struct CommandPattern {
    pub description: String,
    pub usage: String,
    pub examples: Vec<String>,
    pub category: SuggestionCategory,
    pub context_hints: Vec<String>,
}