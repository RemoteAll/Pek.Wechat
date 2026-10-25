//! 微信配置（`Config/Wechat.toml`）。
//!
//! - 配置一律 TOML，PascalCase 键名（与生态内其他 Pek 库一致）；
//! - 读写复用 [`dhrust::config::TomlConfig`]：缺失生成、缺键带注释补齐、类型容错、
//!   损坏备份重建、外部改动热加载；
//! - 数组表 `[[Accounts]]` 的写回复用 [`TomlConfig::save_with_arrays`]（保留注释与排版）；
//! - `AppSecret` 属凭据：[`std::fmt::Debug`] 一律脱敏，不随日志落盘。

use std::path::{Path, PathBuf};

use dhrust::config::TomlConfig;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::WechatError;

/// 默认配置相对路径（相对程序基目录）。
pub const WECHAT_CONFIG_FILE: &str = "Config/Wechat.toml";

/// 配置模板（内嵌；带全部字段中文注释）。
pub const WECHAT_CONFIG_TEMPLATE: &str = include_str!("../res/Wechat.toml.template");

/// 微信账号配置（对应 `[[Accounts]]` 数组表）。
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct WechatAccount {
    /// 惟一标识（调用时按 Code 选择账号）
    pub code: String,
    /// 微信应用 AppId
    pub app_id: String,
    /// 微信应用 AppSecret（凭据，Debug 脱敏）
    pub app_secret: String,
    /// 显示名称
    pub display_name: String,
    /// 授权回调地址（须与微信后台「授权回调域」一致）
    pub redirect_uri: String,
    /// 授权范围：snsapi_login / snsapi_userinfo / snsapi_base
    pub scope: String,
    /// 是否默认账号
    pub is_default: bool,
    /// 是否启用（未启用账号不会被自动选中）
    pub is_enabled: bool,
    /// 选择顺序（越小越优先）
    pub order: i32,
    /// 请求超时（毫秒，默认 10000）
    pub timeout: u64,
    /// 请求重试次数（默认 3，含首次尝试）
    pub retry_times: u32,
    /// 是否跳过 TLS 证书校验（仅自签/内网代理调试）
    pub insecure_tls: bool,
}

impl Default for WechatAccount {
    fn default() -> Self {
        Self {
            code: String::new(),
            app_id: String::new(),
            app_secret: String::new(),
            display_name: String::new(),
            redirect_uri: String::new(),
            scope: "snsapi_login".to_string(),
            is_default: false,
            is_enabled: false,
            order: 0,
            timeout: 10_000,
            retry_times: 3,
            insecure_tls: false,
        }
    }
}

impl std::fmt::Debug for WechatAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WechatAccount")
            .field("code", &self.code)
            .field("app_id", &self.app_id)
            .field("app_secret", &if self.app_secret.is_empty() { "" } else { "***" })
            .field("display_name", &self.display_name)
            .field("redirect_uri", &self.redirect_uri)
            .field("scope", &self.scope)
            .field("is_default", &self.is_default)
            .field("is_enabled", &self.is_enabled)
            .field("order", &self.order)
            .field("timeout", &self.timeout)
            .field("retry_times", &self.retry_times)
            .field("insecure_tls", &self.insecure_tls)
            .finish()
    }
}

impl WechatAccount {
    /// 日志/错误展示用标识（Code 为空时用显示名称，再退到 AppId）。
    pub fn label(&self) -> &str {
        if !self.code.trim().is_empty() {
            &self.code
        } else if !self.display_name.trim().is_empty() {
            &self.display_name
        } else {
            &self.app_id
        }
    }

    /// 请求超时（毫秒；0 按默认 10000）。
    pub fn timeout_ms(&self) -> u64 {
        if self.timeout == 0 {
            crate::client::DEFAULT_TIMEOUT_MS
        } else {
            self.timeout
        }
    }

    /// 总尝试次数（0/1 都按 1 次尝试）。
    pub fn attempts(&self) -> u32 {
        self.retry_times.max(1)
    }

    /// 授权范围（未知值按 `snsapi_login` 处理并告警一次）。
    pub fn login_scope(&self) -> crate::oauth::LoginScope {
        match crate::oauth::LoginScope::from_code(&self.scope) {
            Some(scope) => scope,
            None => {
                dhrust::warn_once!(
                    "微信账号 {} 的 Scope（{}）无法识别，按 snsapi_login 处理",
                    self.label(),
                    self.scope
                );
                crate::oauth::LoginScope::QrConnect
            }
        }
    }

    /// 凭据是否就绪（AppId 与 AppSecret 均非空）。
    pub fn has_credentials(&self) -> bool {
        !self.app_id.trim().is_empty() && !self.app_secret.trim().is_empty()
    }

    /// 按该账号配置创建客户端。
    pub fn client(&self) -> crate::client::WechatClient {
        crate::client::WechatClient::from_account(self)
    }
}

/// 微信设置（`Config/Wechat.toml` 根表）。
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "PascalCase")]
pub struct WechatSettings {
    /// 微信账号列表（`[[Accounts]]`）
    pub accounts: Vec<WechatAccount>,
}

impl WechatSettings {
    /// 默认配置路径（`base/Config/Wechat.toml`）。
    pub fn default_path(base: impl AsRef<Path>) -> PathBuf {
        base.as_ref().join(WECHAT_CONFIG_FILE)
    }

    /// 加载配置（缺失时按内嵌模板生成）。
    pub fn load(path: impl AsRef<Path>) -> TomlConfig<WechatSettings> {
        TomlConfig::load(path, WECHAT_CONFIG_TEMPLATE)
    }

    /// 从程序基目录加载 `Config/Wechat.toml`。
    pub fn load_from_base(base: impl AsRef<Path>) -> TomlConfig<WechatSettings> {
        Self::load(Self::default_path(base))
    }

    /// 按 Code 查找（大小写不敏感）。
    pub fn find_by_code(&self, code: &str) -> Option<&WechatAccount> {
        let code = code.trim();
        self.accounts
            .iter()
            .find(|account| account.code.eq_ignore_ascii_case(code))
    }

    /// 默认账号：`IsDefault` 且已启用者优先，否则 `Order` 最小的已启用账号。
    pub fn default_account(&self) -> Option<&WechatAccount> {
        let mut enabled: Vec<&WechatAccount> =
            self.accounts.iter().filter(|item| item.is_enabled).collect();
        enabled.sort_by_key(|item| (item.order, !item.is_default));
        enabled
            .iter()
            .copied()
            .find(|item| item.is_default)
            .or_else(|| enabled.first().copied())
    }

    /// 选择账号：`code` 为 `None` 时取默认账号。
    ///
    /// 显式指定的账号必须存在且已启用；凭据必须就绪。
    pub fn select(&self, code: Option<&str>) -> Result<&WechatAccount, WechatError> {
        let account = match code.map(str::trim).filter(|value| !value.is_empty()) {
            Some(code) => {
                let account = self
                    .find_by_code(code)
                    .ok_or_else(|| WechatError::config(format!("微信账号 {code} 不存在")))?;
                if !account.is_enabled {
                    return Err(WechatError::config(format!("微信账号 {code} 未启用")));
                }
                account
            }
            None => self.default_account().ok_or_else(|| {
                WechatError::config("没有可用的微信账号（请在 Config/Wechat.toml 启用一个账号）")
            })?,
        };
        if account.app_id.trim().is_empty() {
            return Err(WechatError::config(format!(
                "微信账号 {} 的 AppId 未配置",
                account.label()
            )));
        }
        if account.app_secret.trim().is_empty() {
            return Err(WechatError::config(format!(
                "微信账号 {} 的 AppSecret 未配置",
                account.label()
            )));
        }
        Ok(account)
    }
}

/// 配置文件句柄：持有 [`TomlConfig<WechatSettings>`]，支持保存（含 `[[Accounts]]`）与热加载。
pub struct WechatSettingsFile {
    config: TomlConfig<WechatSettings>,
}

impl WechatSettingsFile {
    /// 按路径加载。
    pub fn load(path: impl AsRef<Path>) -> Self {
        Self {
            config: WechatSettings::load(path),
        }
    }

    /// 从程序基目录加载 `Config/Wechat.toml`。
    pub fn load_from_base(base: impl AsRef<Path>) -> Self {
        Self {
            config: WechatSettings::load_from_base(base),
        }
    }

    /// 配置路径。
    pub fn path(&self) -> &Path {
        self.config.path()
    }

    /// 当前配置。
    pub fn settings(&self) -> &WechatSettings {
        self.config.value()
    }

    /// 当前配置（可变；修改后调用 [`Self::save`]）。
    pub fn settings_mut(&mut self) -> &mut WechatSettings {
        self.config.value_mut()
    }

    /// 是否首次生成（或损坏后重建）。
    pub fn is_new(&self) -> bool {
        self.config.is_new()
    }

    /// 取走加载/保存期间的提示。
    pub fn take_notes(&mut self) -> Vec<String> {
        self.config.take_notes()
    }

    /// 外部修改时热加载；返回提示文本（空 = 未变化）。
    pub fn reload_if_changed(&mut self) -> Vec<String> {
        self.config.reload_if_changed()
    }

    /// 保存配置（标量 + `[[Accounts]]` 数组表；保留注释与排版）。
    pub fn save(&mut self) -> Result<(), WechatError> {
        let entries: Vec<Value> = self
            .config
            .value()
            .accounts
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(|error| WechatError::config(format!("序列化微信账号失败：{error}")))?;
        self.config
            .save_with_arrays(&[("Accounts", &entries)])
            .map_err(|error| WechatError::config(format!("保存微信配置失败：{error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_and_has_one_disabled_account() {
        // 走真实链路：缺失时按内嵌模板生成，再解析回结构体
        let dir = std::env::temp_dir().join(format!("pek-wechat-cfg-{}", dhrust::random::hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let mut file = WechatSettingsFile::load(dir.join("Config/Wechat.toml"));
        assert!(file.is_new());
        let account = file.settings().accounts[0].clone();
        assert_eq!(file.settings().accounts.len(), 1);
        assert_eq!(account.code, "site");
        assert_eq!(account.scope, "snsapi_login");
        assert!(!account.is_enabled);
        assert_eq!(account.login_scope(), crate::oauth::LoginScope::QrConnect);
        assert!(!account.has_credentials());

        // 保存往返：`[[Accounts]]` 写回后可再次解析（注释保留由 TomlConfig 保证）
        file.settings_mut().accounts[0].order = 5;
        file.save().unwrap();
        let reloaded = WechatSettingsFile::load(dir.join("Config/Wechat.toml"));
        assert_eq!(reloaded.settings().accounts[0].order, 5);
        assert!(std::fs::read_to_string(dir.join("Config/Wechat.toml"))
            .unwrap()
            .contains("[[Accounts]]"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn select_prefers_default_then_reports_missing_credentials() {
        let mut settings = WechatSettings::default();
        assert!(matches!(
            settings.select(None).unwrap_err(),
            WechatError::Config(_)
        ));

        settings.accounts = vec![
            WechatAccount {
                code: "a".into(),
                app_id: "wx-a".into(),
                app_secret: "s".into(),
                is_enabled: true,
                order: 20,
                ..WechatAccount::default()
            },
            WechatAccount {
                code: "b".into(),
                app_id: "wx-b".into(),
                app_secret: "s".into(),
                is_enabled: true,
                is_default: true,
                order: 30,
                ..WechatAccount::default()
            },
            WechatAccount {
                code: "off".into(),
                app_id: "wx-off".into(),
                app_secret: "s".into(),
                is_enabled: false,
                order: 1,
                ..WechatAccount::default()
            },
        ];
        // 默认账号优先（即使 Order 更大）
        assert_eq!(settings.select(None).unwrap().code, "b");
        assert_eq!(settings.select(Some("A")).unwrap().code, "a");
        assert!(matches!(
            settings.select(Some("off")).unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(settings
            .select(Some("missing"))
            .unwrap_err()
            .to_string()
            .contains("不存在"));

        // 未配置凭据 → 明确提示
        settings.accounts[1].app_secret.clear();
        let error = settings.select(Some("b")).unwrap_err();
        assert!(error.to_string().contains("AppSecret 未配置"), "{error}");
    }

    #[test]
    fn debug_redacts_app_secret() {
        let account = WechatAccount {
            code: "site".into(),
            app_secret: "super-secret".into(),
            ..WechatAccount::default()
        };
        let text = format!("{account:?}");
        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("site"), "{text}");
    }

    #[test]
    fn unknown_scope_falls_back_to_qr_connect() {
        let account = WechatAccount {
            scope: "snsapi_unknown".into(),
            ..WechatAccount::default()
        };
        assert_eq!(account.login_scope(), crate::oauth::LoginScope::QrConnect);
        let account = WechatAccount {
            scope: "snsapi_userinfo".into(),
            ..WechatAccount::default()
        };
        assert_eq!(account.login_scope(), crate::oauth::LoginScope::UserInfo);
    }
}
