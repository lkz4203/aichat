use std::collections::HashMap;
use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use sha2::{Sha256, Digest};
use hmac::{Hmac, Mac};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, PasswordHashString};
use rand::{Rng, RngCore};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, NewAead};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureStorage {
    encrypted_api_keys: Vec<u8>,
    encrypted_sessions: Vec<u8>,
    key_derivation: Argon2Config,
    master_key: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Argon2Config {
    pub memory_cost: u32,
    pub time_cost: u32,
    pub parallelism: u32,
    pub salt_length: usize,
}

impl Default for Argon2Config {
    fn default() -> Self {
        Self {
            memory_cost: 65536, // 64MB
            time_cost: 3,
            parallelism: 4,
            salt_length: 32,
        }
    }
}

impl SecureStorage {
    pub fn new() -> Self {
        Self {
            encrypted_api_keys: Vec::new(),
            encrypted_sessions: Vec::new(),
            key_derivation: Argon2Config::default(),
            master_key: None,
        }
    }

    pub fn with_config(mut self, config: Argon2Config) -> Self {
        self.key_derivation = config;
        self
    }

    pub async fn initialize(&mut self, master_password: &str) -> Result<()> {
        let salt = self.generate_salt();
        let master_key = self.derive_key(master_password, &salt)?;
        self.master_key = Some(master_key);
        Ok(())
    }

    pub async fn store_api_key(&mut self, provider: &str, api_key: &str) -> Result<()> {
        let master_key = self.master_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Storage not initialized"))?;
        
        let encrypted = self.encrypt_data(api_key.as_bytes(), master_key)?;
        self.encrypted_api_keys = encrypted;
        
        // Log the action
        AuditLogger::log_api_key_action("store", provider, None).await;
        
        Ok(())
    }

    pub async fn retrieve_api_key(&self, provider: &str) -> Result<String> {
        let master_key = self.master_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Storage not initialized"))?;
        
        let decrypted = self.decrypt_data(&self.encrypted_api_keys, master_key)?;
        let api_key = String::from_utf8(decrypted)?;
        
        // Log the action
        AuditLogger::log_api_key_action("retrieve", provider, None).await;
        
        Ok(api_key)
    }

    pub async fn store_session_data(&mut self, session_id: &str, data: &str) -> Result<()> {
        let master_key = self.master_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Storage not initialized"))?;
        
        let encrypted = self.encrypt_data(data.as_bytes(), master_key)?;
        self.encrypted_sessions = encrypted;
        
        // Log the action
        AuditLogger::log_session_action("store", session_id, None).await;
        
        Ok(())
    }

    pub async fn retrieve_session_data(&self, session_id: &str) -> Result<String> {
        let master_key = self.master_key.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Storage not initialized"))?;
        
        let decrypted = self.decrypt_data(&self.encrypted_sessions, master_key)?;
        let data = String::from_utf8(decrypted)?;
        
        // Log the action
        AuditLogger::log_session_action("retrieve", session_id, None).await;
        
        Ok(data)
    }

    fn generate_salt(&self) -> Vec<u8> {
        let mut salt = vec![0u8; self.key_derivation.salt_length];
        rand::thread_rng().fill_bytes(&mut salt);
        salt
    }

    fn derive_key(&self, password: &str, salt: &[u8]) -> Result<Vec<u8>> {
        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(
                self.key_derivation.memory_cost,
                self.key_derivation.time_cost,
                self.key_derivation.parallelism,
                Some(self.key_derivation.salt_length),
            )?,
        );

        let hash = argon2.hash_password(password.as_bytes(), salt)?;
        Ok(hash.hash.unwrap().as_bytes().to_vec())
    }

    fn encrypt_data(&self, data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(key)?;
        let nonce = Aes256Gcm::generate_nonce(&mut rand::thread_rng());
        
        let encrypted = cipher.encrypt(&nonce, data)?;
        let mut result = nonce.to_vec();
        result.extend(encrypted);
        
        Ok(result)
    }

    fn decrypt_data(&self, encrypted_data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        if encrypted_data.len() < 12 {
            return Err(anyhow::anyhow!("Invalid encrypted data"));
        }
        
        let cipher = Aes256Gcm::new_from_slice(key)?;
        let nonce = Nonce::from_slice(&encrypted_data[..12]);
        let ciphertext = &encrypted_data[12..];
        
        let decrypted = cipher.decrypt(nonce, ciphertext)?;
        Ok(decrypted)
    }

    pub fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        let parsed_hash = PasswordHashString::new(hash)?;
        Ok(Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok())
    }

    pub fn hash_password(&self, password: &str) -> Result<String> {
        let salt = self.generate_salt();
        let argon2 = Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(
                self.key_derivation.memory_cost,
                self.key_derivation.time_cost,
                self.key_derivation.parallelism,
                Some(self.key_derivation.salt_length),
            )?,
        );

        let hash = argon2.hash_password(password.as_bytes(), &salt)?;
        Ok(hash.to_string())
    }
}

// Audit Logging System
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallLog {
    pub timestamp: DateTime<Utc>,
    pub provider: String,
    pub endpoint: String,
    pub method: String,
    pub status_code: Option<u16>,
    pub tokens_used: Option<usize>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataAccessLog {
    pub timestamp: DateTime<Utc>,
    pub action: String,
    pub resource_type: String,
    pub resource_id: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub ip_address: Option<String>,
    pub success: bool,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserActionLog {
    pub timestamp: DateTime<Utc>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub action: String,
    pub details: HashMap<String, String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

pub struct AuditLogger {
    api_calls: Arc<RwLock<Vec<ApiCallLog>>>,
    data_access: Arc<RwLock<Vec<DataAccessLog>>>,
    user_actions: Arc<RwLock<Vec<UserActionLog>>>,
    log_file: Option<String>,
    max_log_size: usize,
}

impl AuditLogger {
    pub fn new() -> Self {
        Self {
            api_calls: Arc::new(RwLock::new(Vec::new())),
            data_access: Arc::new(RwLock::new(Vec::new())),
            user_actions: Arc::new(RwLock::new(Vec::new())),
            log_file: None,
            max_log_size: 10000,
        }
    }

    pub fn with_log_file(mut self, log_file: String) -> Self {
        self.log_file = Some(log_file);
        self
    }

    pub fn with_max_size(mut self, max_size: usize) -> Self {
        self.max_log_size = max_size;
        self
    }

    pub async fn log_api_call(
        provider: &str,
        endpoint: &str,
        method: &str,
        status_code: Option<u16>,
        tokens_used: Option<usize>,
        user_id: Option<String>,
        session_id: Option<String>,
    ) {
        let log = ApiCallLog {
            timestamp: Utc::now(),
            provider: provider.to_string(),
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            status_code,
            tokens_used,
            user_id,
            session_id,
            ip_address: None, // Would be set from request context
            user_agent: None, // Would be set from request context
        };

        let mut api_calls = AuditLogger::get_instance().api_calls.write().await;
        api_calls.push(log);
        
        // Trim if necessary
        if api_calls.len() > AuditLogger::get_instance().max_log_size {
            api_calls.drain(0..api_calls.len() - AuditLogger::get_instance().max_log_size);
        }
    }

    pub async fn log_data_access(
        action: &str,
        resource_type: &str,
        resource_id: &str,
        user_id: Option<String>,
        session_id: Option<String>,
        success: bool,
        error_message: Option<String>,
    ) {
        let log = DataAccessLog {
            timestamp: Utc::now(),
            action: action.to_string(),
            resource_type: resource_type.to_string(),
            resource_id: resource_id.to_string(),
            user_id,
            session_id,
            ip_address: None, // Would be set from request context
            success,
            error_message,
        };

        let mut data_access = AuditLogger::get_instance().data_access.write().await;
        data_access.push(log);
        
        // Trim if necessary
        if data_access.len() > AuditLogger::get_instance().max_log_size {
            data_access.drain(0..data_access.len() - AuditLogger::get_instance().max_log_size);
        }
    }

    pub async fn log_user_action(
        user_id: Option<String>,
        session_id: Option<String>,
        action: &str,
        details: HashMap<String, String>,
    ) {
        let log = UserActionLog {
            timestamp: Utc::now(),
            user_id,
            session_id,
            action: action.to_string(),
            details,
            ip_address: None, // Would be set from request context
            user_agent: None, // Would be set from request context
        };

        let mut user_actions = AuditLogger::get_instance().user_actions.write().await;
        user_actions.push(log);
        
        // Trim if necessary
        if user_actions.len() > AuditLogger::get_instance().max_log_size {
            user_actions.drain(0..user_actions.len() - AuditLogger::get_instance().max_log_size);
        }
    }

    async fn log_api_key_action(action: &str, provider: &str, user_id: Option<String>) {
        let mut details = HashMap::new();
        details.insert("provider".to_string(), provider.to_string());
        details.insert("action".to_string(), action.to_string());
        
        Self::log_user_action(user_id, None, "api_key_access", details).await;
    }

    async fn log_session_action(action: &str, session_id: &str, user_id: Option<String>) {
        let mut details = HashMap::new();
        details.insert("session_id".to_string(), session_id.to_string());
        details.insert("action".to_string(), action.to_string());
        
        Self::log_user_action(user_id, Some(session_id.to_string()), "session_access", details).await;
    }

    pub async fn get_api_calls(&self, limit: Option<usize>) -> Vec<ApiCallLog> {
        let api_calls = self.api_calls.read().await;
        let limit = limit.unwrap_or(api_calls.len());
        api_calls.iter().rev().take(limit).cloned().collect()
    }

    pub async fn get_data_access_logs(&self, limit: Option<usize>) -> Vec<DataAccessLog> {
        let data_access = self.data_access.read().await;
        let limit = limit.unwrap_or(data_access.len());
        data_access.iter().rev().take(limit).cloned().collect()
    }

    pub async fn get_user_actions(&self, limit: Option<usize>) -> Vec<UserActionLog> {
        let user_actions = self.user_actions.read().await;
        let limit = limit.unwrap_or(user_actions.len());
        user_actions.iter().rev().take(limit).cloned().collect()
    }

    pub async fn export_logs(&self, format: ExportFormat) -> Result<String> {
        match format {
            ExportFormat::Json => {
                let api_calls = self.get_api_calls(None).await;
                let data_access = self.get_data_access_logs(None).await;
                let user_actions = self.get_user_actions(None).await;
                
                let export = LogExport {
                    api_calls,
                    data_access,
                    user_actions,
                    export_timestamp: Utc::now(),
                };
                
                Ok(serde_json::to_string_pretty(&export)?)
            }
            ExportFormat::Csv => {
                // Implement CSV export
                Ok("CSV export not implemented yet".to_string())
            }
        }
    }

    pub async fn clear_logs(&self) {
        self.api_calls.write().await.clear();
        self.data_access.write().await.clear();
        self.user_actions.write().await.clear();
    }

    fn get_instance() -> &'static AuditLogger {
        static INSTANCE: once_cell::sync::Lazy<AuditLogger> = once_cell::sync::Lazy::new(|| {
            AuditLogger::new()
        });
        &INSTANCE
    }
}

#[derive(Debug, Clone)]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogExport {
    pub api_calls: Vec<ApiCallLog>,
    pub data_access: Vec<DataAccessLog>,
    pub user_actions: Vec<UserActionLog>,
    pub export_timestamp: DateTime<Utc>,
}

// Security utilities
pub struct SecurityUtils;

impl SecurityUtils {
    pub fn generate_secure_token() -> String {
        let mut token = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut token);
        base64::encode(token)
    }

    pub fn hash_string(input: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn verify_hmac(data: &[u8], key: &[u8], signature: &str) -> bool {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
        mac.update(data);
        mac.verify_slice(&base64::decode(signature).unwrap_or_default()).is_ok()
    }

    pub fn generate_hmac(data: &[u8], key: &[u8]) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(key).unwrap();
        mac.update(data);
        base64::encode(mac.finalize().into_bytes())
    }

    pub fn sanitize_input(input: &str) -> String {
        // Basic input sanitization
        input
            .replace("<script>", "")
            .replace("javascript:", "")
            .replace("data:", "")
            .trim()
            .to_string()
    }

    pub fn validate_api_key_format(api_key: &str) -> bool {
        // Basic validation - in practice you'd want more sophisticated validation
        !api_key.is_empty() && api_key.len() >= 10
    }
}