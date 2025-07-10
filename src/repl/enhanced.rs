use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use reedline::{DefaultPrompt, Reedline, Signal};
use serde::{Deserialize, Serialize};
use syntect::easy::HighlightLines;
use syntect::highlighting::{ThemeSet, SyntaxSet};
use syntect::util::{as_24_bit_terminal_escaped, LinesWithEndings};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedReplConfig {
    pub syntax_highlighting: bool,
    pub auto_completion: bool,
    pub command_history: bool,
    pub custom_keybindings: HashMap<String, String>,
    pub multi_line_editing: bool,
    pub history_size: usize,
    pub auto_save_history: bool,
    pub history_file: Option<String>,
    pub theme: String,
    pub prompt_style: PromptStyle,
    pub completion_style: CompletionStyle,
}

impl Default for EnhancedReplConfig {
    fn default() -> Self {
        Self {
            syntax_highlighting: true,
            auto_completion: true,
            command_history: true,
            custom_keybindings: HashMap::new(),
            multi_line_editing: true,
            history_size: 1000,
            auto_save_history: true,
            history_file: Some("~/.aichat/history.txt".to_string()),
            theme: "monokai".to_string(),
            prompt_style: PromptStyle::default(),
            completion_style: CompletionStyle::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptStyle {
    pub left_prompt: String,
    pub right_prompt: String,
    pub colors: HashMap<String, String>,
    pub show_git_branch: bool,
    pub show_current_dir: bool,
    pub show_session_info: bool,
}

impl Default for PromptStyle {
    fn default() -> Self {
        Self {
            left_prompt: "{color.green}{?session {session}>}{!session >}{color.reset} ".to_string(),
            right_prompt: "{color.purple}{?session {consume_tokens}({consume_percent}%)}{color.reset}".to_string(),
            colors: HashMap::new(),
            show_git_branch: true,
            show_current_dir: true,
            show_session_info: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionStyle {
    pub show_descriptions: bool,
    pub max_suggestions: usize,
    pub fuzzy_matching: bool,
    pub case_sensitive: bool,
    pub sort_by_usage: bool,
}

impl Default for CompletionStyle {
    fn default() -> Self {
        Self {
            show_descriptions: true,
            max_suggestions: 10,
            fuzzy_matching: true,
            case_sensitive: false,
            sort_by_usage: true,
        }
    }
}

pub struct EnhancedRepl {
    config: EnhancedReplConfig,
    reedline: Reedline,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    history: Vec<String>,
    command_usage: HashMap<String, u64>,
    custom_commands: HashMap<String, Box<dyn Fn(&[String]) -> Result<String> + Send + Sync>>,
}

impl EnhancedRepl {
    pub fn new(config: EnhancedReplConfig) -> Result<Self> {
        let mut reedline = Reedline::create()?;
        
        // Load syntax highlighting
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        
        // Setup custom keybindings
        Self::setup_keybindings(&mut reedline, &config.custom_keybindings)?;
        
        Ok(Self {
            config,
            reedline,
            syntax_set,
            theme_set,
            history: Vec::new(),
            command_usage: HashMap::new(),
            custom_commands: HashMap::new(),
        })
    }

    fn setup_keybindings(
        reedline: &mut Reedline,
        keybindings: &HashMap<String, String>,
    ) -> Result<()> {
        for (key, action) in keybindings {
            let key_event = Self::parse_key_event(key)?;
            // Note: This is a simplified implementation. In a real implementation,
            // you would need to properly integrate with reedline's keybinding system
            log::debug!("Registered keybinding: {:?} -> {}", key_event, action);
        }
        Ok(())
    }

    fn parse_key_event(key_str: &str) -> Result<KeyEvent> {
        // Parse key event from string representation
        // This is a simplified parser - you'd want a more robust implementation
        let parts: Vec<&str> = key_str.split('-').collect();
        
        let mut modifiers = KeyModifiers::empty();
        let mut key_code = KeyCode::Char(' ');
        
        for part in parts {
            match part.to_lowercase().as_str() {
                "ctrl" => modifiers |= KeyModifiers::CONTROL,
                "alt" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                "super" => modifiers |= KeyModifiers::SUPER,
                _ => {
                    // Parse key code
                    if part.len() == 1 {
                        key_code = KeyCode::Char(part.chars().next().unwrap());
                    } else {
                        key_code = match part {
                            "enter" => KeyCode::Enter,
                            "tab" => KeyCode::Tab,
                            "backspace" => KeyCode::Backspace,
                            "delete" => KeyCode::Delete,
                            "up" => KeyCode::Up,
                            "down" => KeyCode::Down,
                            "left" => KeyCode::Left,
                            "right" => KeyCode::Right,
                            "home" => KeyCode::Home,
                            "end" => KeyCode::End,
                            "pageup" => KeyCode::PageUp,
                            "pagedown" => KeyCode::PageDown,
                            "escape" => KeyCode::Esc,
                            "f1" => KeyCode::F(1),
                            "f2" => KeyCode::F(2),
                            "f3" => KeyCode::F(3),
                            "f4" => KeyCode::F(4),
                            "f5" => KeyCode::F(5),
                            "f6" => KeyCode::F(6),
                            "f7" => KeyCode::F(7),
                            "f8" => KeyCode::F(8),
                            "f9" => KeyCode::F(9),
                            "f10" => KeyCode::F(10),
                            "f11" => KeyCode::F(11),
                            "f12" => KeyCode::F(12),
                            _ => return Err(anyhow::anyhow!("Unknown key: {}", part)),
                        };
                    }
                }
            }
        }
        
        Ok(KeyEvent::new(key_code, modifiers))
    }

    pub fn highlight_syntax(&self, code: &str, language: &str) -> String {
        if !self.config.syntax_highlighting {
            return code.to_string();
        }

        let syntax = self.syntax_set
            .find_syntax_by_name(language)
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());
        
        let theme = self.theme_set
            .themes
            .get(&self.config.theme)
            .unwrap_or_else(|| self.theme_set.themes.get("base16-ocean.dark").unwrap());
        
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut highlighted = String::new();
        
        for (line_number, line) in LinesWithEndings::from(code).enumerate() {
            let ranges: Vec<(syntect::highlighting::Style, &str)> = highlighter
                .highlight_line(line, &self.syntax_set)
                .unwrap_or_else(|_| vec![(syntect::highlighting::Style::default(), line)]);
            
            highlighted.push_str(&as_24_bit_terminal_escaped(&ranges[..], false));
        }
        
        highlighted
    }

    pub fn add_custom_command<F>(&mut self, name: String, handler: F)
    where
        F: Fn(&[String]) -> Result<String> + Send + Sync + 'static,
    {
        self.custom_commands.insert(name, Box::new(handler));
    }

    pub fn get_completions(&self, input: &str) -> Vec<(String, Option<String>)> {
        if !self.config.auto_completion {
            return Vec::new();
        }

        let mut completions = Vec::new();
        
        // Built-in commands
        let builtin_commands = vec![
            ("help", "Show help information"),
            ("exit", "Exit the REPL"),
            ("clear", "Clear the screen"),
            ("history", "Show command history"),
            ("config", "Show current configuration"),
            ("session", "Session management"),
            ("role", "Role management"),
            ("rag", "RAG operations"),
            ("agent", "Agent operations"),
        ];

        for (cmd, desc) in builtin_commands {
            if cmd.starts_with(input) {
                completions.push((cmd.to_string(), Some(desc.to_string())));
            }
        }

        // Custom commands
        for (cmd, _) in &self.custom_commands {
            if cmd.starts_with(input) {
                completions.push((cmd.clone(), Some("Custom command".to_string())));
            }
        }

        // Fuzzy matching if enabled
        if self.config.completion_style.fuzzy_matching {
            let fuzzy_completions = self.get_fuzzy_completions(input);
            completions.extend(fuzzy_completions);
        }

        // Sort by usage if enabled
        if self.config.completion_style.sort_by_usage {
            completions.sort_by(|a, b| {
                let usage_a = self.command_usage.get(&a.0).unwrap_or(&0);
                let usage_b = self.command_usage.get(&b.0).unwrap_or(&0);
                usage_b.cmp(usage_a)
            });
        }

        // Limit suggestions
        completions.truncate(self.config.completion_style.max_suggestions);
        
        completions
    }

    fn get_fuzzy_completions(&self, input: &str) -> Vec<(String, Option<String>)> {
        use fuzzy_matcher::{FuzzyMatcher, SkimMatcherV2};
        
        let matcher = SkimMatcherV2::default();
        let mut fuzzy_completions = Vec::new();
        
        // Add fuzzy matches for all available commands
        let all_commands = vec![
            "help", "exit", "clear", "history", "config", "session", 
            "role", "rag", "agent", "macro", "function", "tool"
        ];
        
        for cmd in all_commands {
            if let Some(score) = matcher.fuzzy_match(cmd, input) {
                fuzzy_completions.push((cmd.to_string(), Some(format!("Fuzzy match (score: {})", score))));
            }
        }
        
        fuzzy_completions.sort_by(|a, b| {
            // Sort by fuzzy match score (higher is better)
            let score_a = a.1.as_ref().unwrap().split('(').nth(1).unwrap_or("0").trim_end_matches(')');
            let score_b = b.1.as_ref().unwrap().split('(').nth(1).unwrap_or("0").trim_end_matches(')');
            score_b.parse::<i64>().unwrap_or(0).cmp(&score_a.parse::<i64>().unwrap_or(0))
        });
        
        fuzzy_completions
    }

    pub fn add_to_history(&mut self, command: String) {
        if self.config.command_history {
            // Avoid duplicates
            if !self.history.contains(&command) {
                self.history.push(command.clone());
                
                // Limit history size
                if self.history.len() > self.config.history_size {
                    self.history.remove(0);
                }
            }
            
            // Update usage statistics
            *self.command_usage.entry(command).or_insert(0) += 1;
        }
    }

    pub fn get_history(&self) -> &[String] {
        &self.history
    }

    pub fn search_history(&self, query: &str) -> Vec<String> {
        self.history
            .iter()
            .filter(|cmd| cmd.contains(query))
            .cloned()
            .collect()
    }

    pub fn render_prompt(&self, context: &PromptContext) -> String {
        let mut prompt = self.config.prompt_style.left_prompt.clone();
        
        // Replace placeholders
        prompt = prompt.replace("{color.green}", "\x1b[32m");
        prompt = prompt.replace("{color.reset}", "\x1b[0m");
        prompt = prompt.replace("{color.purple}", "\x1b[35m");
        
        if let Some(session) = &context.session {
            prompt = prompt.replace("{session}", session);
        } else {
            prompt = prompt.replace("{?session {session}>}", "");
            prompt = prompt.replace("{!session >}", ">");
        }
        
        if let Some(role) = &context.role {
            prompt = prompt.replace("{role}", role);
        }
        
        if let Some(rag) = &context.rag {
            prompt = prompt.replace("{rag}", rag);
        }
        
        if let Some(tokens) = context.consume_tokens {
            prompt = prompt.replace("{consume_tokens}", &tokens.to_string());
        }
        
        if let Some(percent) = context.consume_percent {
            prompt = prompt.replace("{consume_percent}", &percent.to_string());
        }
        
        prompt
    }

    pub async fn read_line(&mut self, context: &PromptContext) -> Result<Option<String>> {
        let prompt = self.render_prompt(context);
        let prompt = DefaultPrompt::new(prompt);
        
        match self.reedline.read_line(&prompt)? {
            Signal::Success(line) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    self.add_to_history(trimmed.to_string());
                }
                Ok(Some(trimmed.to_string()))
            }
            Signal::CtrlD => Ok(None),
            Signal::CtrlC => Ok(None),
            _ => Ok(None),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptContext {
    pub session: Option<String>,
    pub role: Option<String>,
    pub rag: Option<String>,
    pub consume_tokens: Option<usize>,
    pub consume_percent: Option<f64>,
    pub current_dir: Option<String>,
    pub git_branch: Option<String>,
}

impl Default for PromptContext {
    fn default() -> Self {
        Self {
            session: None,
            role: None,
            rag: None,
            consume_tokens: None,
            consume_percent: None,
            current_dir: None,
            git_branch: None,
        }
    }
}

// Multi-line editing support
pub struct MultiLineEditor {
    lines: Vec<String>,
    current_line: usize,
    prompt: String,
}

impl MultiLineEditor {
    pub fn new(prompt: String) -> Self {
        Self {
            lines: Vec::new(),
            current_line: 0,
            prompt,
        }
    }

    pub fn add_line(&mut self, line: String) {
        self.lines.push(line);
        self.current_line = self.lines.len();
    }

    pub fn get_content(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_complete(&self) -> bool {
        // Simple heuristic: check if the last line ends with a semicolon or is empty
        if let Some(last_line) = self.lines.last() {
            last_line.trim().is_empty() || last_line.trim().ends_with(';')
        } else {
            false
        }
    }

    pub fn render(&self) -> String {
        let mut output = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i == self.current_line {
                output.push_str(&format!("{}> {}", self.prompt, line));
            } else {
                output.push_str(&format!("  {}", line));
            }
            output.push('\n');
        }
        output
    }
}

// Command history with persistence
pub struct CommandHistory {
    history: Vec<String>,
    max_size: usize,
    file_path: Option<String>,
}

impl CommandHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            history: Vec::new(),
            max_size,
            file_path: None,
        }
    }

    pub fn with_file_path(mut self, file_path: String) -> Self {
        self.file_path = Some(file_path);
        self
    }

    pub fn add(&mut self, command: String) {
        // Remove if already exists (to move to end)
        self.history.retain(|cmd| cmd != &command);
        self.history.push(command);
        
        // Limit size
        if self.history.len() > self.max_size {
            self.history.remove(0);
        }
    }

    pub fn search(&self, query: &str) -> Vec<String> {
        self.history
            .iter()
            .filter(|cmd| cmd.contains(query))
            .cloned()
            .collect()
    }

    pub fn load(&mut self) -> Result<()> {
        if let Some(ref path) = self.file_path {
            if let Ok(content) = std::fs::read_to_string(path) {
                self.history = content.lines().map(|s| s.to_string()).collect();
            }
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        if let Some(ref path) = self.file_path {
            if let Some(parent) = std::path::Path::new(path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, self.history.join("\n"))?;
        }
        Ok(())
    }
}