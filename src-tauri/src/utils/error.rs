use serde::Serialize;

/// フロントエンドへ返すエラー。`code` で分岐し、`message` はAPIキー等を含まない。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid provider: {0}")]
    InvalidProvider(String),
    /// どのAPI(プロバイダー)で起きたエラーかを付与する。
    #[error("{inner}")]
    Source { api: &'static str, inner: Box<AppError> },
    #[error("API key is empty")]
    EmptyKey,
    #[error("API key was rejected by the provider")]
    Unauthorized,
    #[error("rate limited or quota exceeded")]
    RateLimited,
    #[error("request timed out")]
    Timeout,
    #[error("provider server error ({0})")]
    Server(u16),
    #[error("network error")]
    Network,
    #[error("secure storage error: {0}")]
    SecretStore(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("no API key registered for any provider")]
    NoProvider,
    #[error("API key for {0} is not registered")]
    KeyMissing(String),
    #[error("unknown model: {0}")]
    InvalidModel(String),
    #[error("message is empty")]
    EmptyMessage,
    #[error("another request is still running in this workspace")]
    Busy,
    #[error("unexpected response from provider")]
    BadResponse,
    #[error("audio error: {0}")]
    Audio(String),
    #[error("already recording")]
    AlreadyRecording,
    #[error("not recording")]
    NotRecording,
    #[error("no speech was captured")]
    NoAudio,
    #[error("recording was too short")]
    TooShort,
    #[error("recording is too long for Google speech recognition")]
    TooLong,
    #[error("microphone delivered no signal (permission denied or muted)")]
    MicNoSignal,
    #[error("microphone input was too quiet")]
    MicTooQuiet,
    #[error("speech recognition model is not installed")]
    SttModelMissing,
    #[error("speech recognition failed: {0}")]
    Stt(String),
    #[error("text is empty")]
    EmptyText,
    #[error("VOICEVOX CORE failed: {0}")]
    Voicevox(String),
    #[error("VOICEVOX CORE is not installed")]
    VoicevoxMissing,
    /// 端末内のLLM(llama-serverとモデル)が未取得。
    #[error("local LLM is not installed")]
    LocalLlmMissing,
    /// 端末内のLLMの起動・応答の失敗(原因の文面はllama-serverのログや応答から取る)。
    #[error("local LLM failed: {0}")]
    LocalLlm(String),
    /// 取得したファイルのサイズかSHA-256が、固定した値と一致しない。
    #[error("downloaded file failed verification")]
    ChecksumMismatch,
    /// 取得を中断した。
    #[error("download was cancelled")]
    DownloadCancelled,
    #[error("invalid version: {0}")]
    InvalidVersion(String),
    #[error("path is outside the knowledge directory")]
    PathNotAllowed,
    /// プロバイダーが理由付きで拒否した場合(原因特定用。本文にキーは含まれない)。
    #[error("{0}")]
    ProviderRejected(String),
    #[error("the API is not enabled for this key's project")]
    ApiDisabled,
    #[error("the API key's restrictions block this request")]
    KeyRestricted,
    #[error("billing is not enabled for the project")]
    BillingDisabled,
    #[error("knowledge file I/O error: {0}")]
    Io(String),
    #[error("invalid settings file: {0}")]
    InvalidSettings(String),
    /// バックアップのzipが使えない(形式・版・チェックサム・不正なパス)。
    #[error("invalid backup: {0}")]
    InvalidBackup(String),
    /// 暗号化されたバックアップで、パスワードが無い。
    #[error("backup password is required")]
    BackupPasswordRequired,
    /// バックアップのパスワードが違う。
    #[error("backup password is incorrect")]
    BackupPasswordWrong,
    #[error("invalid pronunciation dictionary: {0}")]
    InvalidDictionary(String),
    #[error("invalid personal data: {0}")]
    InvalidProfile(String),
    /// エージェントデータ・共通データの不備(上限超過、使えない文字)。
    #[error("invalid agent data: {0}")]
    InvalidAgentData(String),
    /// スキルの不備(名前・長さ・同名・見つからない、など)。メッセージは利用者(とLLM)に見せる文面。
    #[error("invalid skill: {0}")]
    InvalidSkill(String),
    /// 貼り付けたAWS設定の不備(プロファイルが無い、キーが無い、MFAコードの形式不正など)。
    #[error("{0}")]
    AwsConfig(String),
    /// MFAコードの入力が取り消された、または時間内に入力されなかった。
    #[error("MFA code was not provided")]
    MfaRequired,
    /// SSOのブラウザでの承認が取り消された、または時間内に終わらなかった。
    #[error("SSO sign-in was not completed")]
    SsoRequired,
    /// エージェントの名称・イメージカラーの不備。
    #[error("invalid agent: {0}")]
    InvalidAgent(String),
    /// 指定した保存先が、別のワークスペースの保存先(ナレッジ・デバッグログ)と重なっている。
    #[error("path overlaps another workspace's location")]
    PathInUse,
    /// 名称がパーソナル・ワークの表示名と紛らわしいため使えない。
    #[error("agent name is reserved")]
    ReservedName,
    /// 指定した名称を、別のエージェントがすでに使っている。
    #[error("agent name already in use")]
    NameInUse,
    /// 指定した色を、別のエージェントがすでに使っている。
    #[error("agent color already in use")]
    ColorInUse,
    /// エージェント数が上限に達している。
    #[error("agent limit reached")]
    AgentLimit,
    /// 指定したエージェントが無い。
    #[error("agent not found")]
    AgentNotFound,
    /// エージェントを作る前に、リーダーの作成が必要。
    #[error("leader is required")]
    LeaderRequired,
    /// リーダーはすでにある(1つだけ)。
    #[error("leader already exists")]
    LeaderExists,
    /// ワークが無効になっている(リーダーとエージェントは使えない)。
    #[error("work is disabled")]
    WorkDisabled,
    /// ワークの中に実行中・承認待ちの依頼があり、無効にできない。
    #[error("work is busy")]
    WorkBusy,
    /// サンドボックス(Docker)の不備。メッセージは利用者に見せる文面。
    #[error("{0}")]
    Sandbox(String),
}

impl AppError {
    /// エラーの発生元APIを付与する(すでに付与済みなら元の発生元を維持)。
    pub fn with_source(self, source: &'static str) -> Self {
        match self {
            Self::Source { .. } => self,
            inner => Self::Source { api: source, inner: Box::new(inner) },
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Source { inner, .. } => inner.code(),
            Self::InvalidProvider(_) => "invalid_provider",
            Self::EmptyKey => "empty_key",
            Self::Unauthorized => "unauthorized",
            Self::RateLimited => "rate_limited",
            Self::Timeout => "timeout",
            Self::Server(_) => "server_error",
            Self::Network => "network_error",
            Self::SecretStore(_) => "secret_store_error",
            Self::Db(_) => "db_error",
            Self::NoProvider => "no_provider",
            Self::KeyMissing(_) => "key_missing",
            Self::InvalidModel(_) => "invalid_model",
            Self::EmptyMessage => "empty_message",
            Self::Busy => "busy",
            Self::BadResponse => "bad_response",
            Self::Audio(_) => "audio_error",
            Self::AlreadyRecording => "already_recording",
            Self::NotRecording => "not_recording",
            Self::NoAudio => "no_audio",
            Self::TooShort => "too_short",
            Self::TooLong => "too_long",
            Self::MicNoSignal => "mic_no_signal",
            Self::MicTooQuiet => "mic_too_quiet",
            Self::SttModelMissing => "stt_model_missing",
            Self::Stt(_) => "stt_error",
            Self::EmptyText => "empty_text",
            Self::Voicevox(_) => "voicevox_error",
            Self::VoicevoxMissing => "voicevox_missing",
            Self::LocalLlmMissing => "local_llm_missing",
            Self::LocalLlm(_) => "local_llm_error",
            Self::ChecksumMismatch => "checksum_mismatch",
            Self::DownloadCancelled => "download_cancelled",
            Self::InvalidVersion(_) => "invalid_version",
            Self::PathNotAllowed => "path_not_allowed",
            Self::Io(_) => "io_error",
            Self::InvalidSettings(_) => "invalid_settings",
            Self::InvalidBackup(_) => "invalid_backup",
            Self::BackupPasswordRequired => "backup_password_required",
            Self::BackupPasswordWrong => "backup_password_wrong",
            Self::InvalidDictionary(_) => "invalid_dictionary",
            Self::InvalidProfile(_) => "invalid_profile",
            Self::InvalidAgentData(_) => "invalid_agent_data",
            Self::InvalidSkill(_) => "invalid_skill",
            Self::AwsConfig(_) => "aws_config_error",
            Self::MfaRequired => "mfa_required",
            Self::SsoRequired => "sso_required",
            Self::InvalidAgent(_) => "invalid_agent",
            Self::PathInUse => "path_in_use",
            Self::ReservedName => "reserved_name",
            Self::NameInUse => "name_in_use",
            Self::ColorInUse => "color_in_use",
            Self::AgentLimit => "agent_limit",
            Self::AgentNotFound => "agent_not_found",
            Self::LeaderRequired => "leader_required",
            Self::LeaderExists => "leader_exists",
            Self::WorkDisabled => "work_disabled",
            Self::WorkBusy => "work_busy",
            Self::Sandbox(_) => "sandbox_error",
            Self::ProviderRejected(_) => "provider_rejected",
            Self::ApiDisabled => "api_disabled",
            Self::KeyRestricted => "key_restricted",
            Self::BillingDisabled => "billing_disabled",
        }
    }
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    code: &'a str,
    message: String,
    /// 発生元API("anthropic" | "bedrock" | "local" | "google_tts" | "google_stt" | "voicevox")。不明な場合はnull。
    source: Option<&'static str>,
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let source = match self {
            Self::Source { api, .. } => Some(*api),
            _ => None,
        };
        ErrorPayload { code: self.code(), message: self.to_string(), source }.serialize(s)
    }
}

impl AppError {
    pub fn from_status(status: u16) -> Self {
        match status {
            400 | 401 | 403 => Self::Unauthorized,
            429 => Self::RateLimited,
            s => Self::Server(s),
        }
    }

    /// Google APIのエラー本文の`reason`から原因を細分化する(キー自体は本文に含まれない)。
    pub fn from_google(status: u16, body: &str) -> Self {
        let v = serde_json::from_str::<serde_json::Value>(body).ok();
        let reason = v.as_ref().and_then(|v| {
            v.pointer("/error/details")?
                .as_array()?
                .iter()
                .find_map(|d| d.get("reason")?.as_str().map(str::to_owned))
        });
        let message = v
            .as_ref()
            .and_then(|v| v.pointer("/error/message")?.as_str().map(|m| m.chars().take(300).collect::<String>()));
        match (reason.as_deref(), message) {
            (Some("SERVICE_DISABLED"), _) => Self::ApiDisabled,
            (Some("BILLING_DISABLED"), _) => Self::BillingDisabled,
            (Some(r), _) if r.ends_with("_BLOCKED") => Self::KeyRestricted,
            (Some("API_KEY_INVALID"), _) => Self::Unauthorized,
            (reason, Some(m)) if matches!(status, 400 | 401 | 403) => {
                Self::ProviderRejected(format!("{} (HTTP {status}{})", m, reason.map(|r| format!(", {r}")).unwrap_or_default()))
            }
            _ => Self::from_status(status),
        }
    }

    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() { Self::Timeout } else { Self::Network }
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_is_attached_once_and_serialized() {
        let e = AppError::Unauthorized.with_source("anthropic").with_source("bedrock");
        assert_eq!(e.code(), "unauthorized");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["source"], "anthropic");
        assert_eq!(v["code"], "unauthorized");
        assert!(serde_json::to_value(AppError::EmptyKey).unwrap()["source"].is_null());
    }

    #[test]
    fn google_reason_mapping() {
        let body = |r: &str| format!(r#"{{"error":{{"details":[{{"reason":"{r}"}}]}}}}"#);
        assert!(matches!(AppError::from_google(403, &body("SERVICE_DISABLED")), AppError::ApiDisabled));
        assert!(matches!(AppError::from_google(403, &body("API_KEY_SERVICE_BLOCKED")), AppError::KeyRestricted));
        assert!(matches!(AppError::from_google(403, &body("BILLING_DISABLED")), AppError::BillingDisabled));
        assert!(matches!(AppError::from_google(400, &body("API_KEY_INVALID")), AppError::Unauthorized));
        assert!(matches!(AppError::from_google(429, "not json"), AppError::RateLimited));
        let e = AppError::from_google(403, r#"{"error":{"message":"Method doesn't allow unregistered callers"}}"#);
        assert!(matches!(&e, AppError::ProviderRejected(m) if m.contains("unregistered") && m.contains("403")));
    }
}
