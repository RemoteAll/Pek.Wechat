//! 配置驱动的统一入口（对齐 `Pek.RSms` 的 `Smser`）。
//!
//! ```no_run
//! use pek_wechat::Wechater;
//!
//! let wechater = Wechater::open_in(".");
//! let client = wechater.client(None)?; // 取默认账号
//! let state = pek_wechat::new_state();
//! let url = client.qr_connect_url("https://example.com/auth/wechat/callback", &state);
//! println!("跳转：{url}");
//! # Ok::<(), pek_wechat::WechatError>(())
//! ```

use std::path::Path;

use crate::client::WechatClient;
use crate::error::WechatError;
use crate::oauth;
use crate::settings::{WechatSettings, WechatSettingsFile};

/// 微信统一入口：加载/维护 `Config/Wechat.toml`，按账号创建客户端。
pub struct Wechater {
    file: WechatSettingsFile,
}

impl Wechater {
    /// 从程序基目录加载 `Config/Wechat.toml`（缺失时按模板生成）。
    pub fn open_in(base: impl AsRef<Path>) -> Self {
        Self {
            file: WechatSettingsFile::load_from_base(base),
        }
    }

    /// 按显式路径加载配置。
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self {
            file: WechatSettingsFile::load(path),
        }
    }

    /// 配置路径。
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// 当前配置。
    pub fn settings(&self) -> &WechatSettings {
        self.file.settings()
    }

    /// 当前配置（可变；修改后调用 [`Self::save`]）。
    pub fn settings_mut(&mut self) -> &mut WechatSettings {
        self.file.settings_mut()
    }

    /// 是否首次生成（或损坏后重建）。
    pub fn is_new(&self) -> bool {
        self.file.is_new()
    }

    /// 取走加载/保存期间的提示（缺键补齐、类型修正、损坏备份等）。
    pub fn take_notes(&mut self) -> Vec<String> {
        self.file.take_notes()
    }

    /// 外部修改时热加载；返回提示文本（空 = 未变化）。
    pub fn reload_if_changed(&mut self) -> Vec<String> {
        self.file.reload_if_changed()
    }

    /// 保存配置（含 `[[Accounts]]`；保留注释与排版）。
    pub fn save(&mut self) -> Result<(), WechatError> {
        self.file.save()
    }

    /// 按 Code 创建客户端（`None` = 默认账号）。
    pub fn client(&self, code: Option<&str>) -> Result<WechatClient, WechatError> {
        Ok(self.settings().select(code)?.client())
    }

    /// 默认账号的客户端。
    pub fn default_client(&self) -> Result<WechatClient, WechatError> {
        self.client(None)
    }

    /// 按账号配置构造授权地址（回调地址取账号的 `RedirectUri`）。
    ///
    /// - 网站应用扫码登录（`Scope=snsapi_login`）→ `connect/qrconnect`
    /// - 公众号网页授权（`snsapi_userinfo` / `snsapi_base`）→ `connect/oauth2/authorize`
    pub fn authorize_url(&self, code: Option<&str>, state: &str) -> Result<String, WechatError> {
        let account = self.settings().select(code)?;
        if account.redirect_uri.trim().is_empty() {
            return Err(WechatError::config(format!(
                "微信账号 {} 的 RedirectUri 未配置",
                account.label()
            )));
        }
        Ok(oauth::authorize_url(
            &account.app_id,
            &account.redirect_uri,
            account.login_scope(),
            state,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_in_generates_template_and_reports_notes() {
        let dir = std::env::temp_dir().join(format!("pek-wechat-entry-{}", dhrust::random::hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let mut wechater = Wechater::open_in(&dir);
        assert!(wechater.is_new());
        assert!(wechater.path().ends_with("Wechat.toml"));
        assert_eq!(wechater.settings().accounts.len(), 1);
        let _ = wechater.take_notes();

        // 默认账号未启用 → 明确报错（不会静默发起网络请求）
        let error = wechater.default_client().unwrap_err();
        assert!(matches!(error, WechatError::Config(_)), "{error}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn authorize_url_requires_redirect_uri() {
        let dir = std::env::temp_dir().join(format!("pek-wechat-entry-{}", dhrust::random::hex(6)));
        std::fs::create_dir_all(&dir).unwrap();
        let mut wechater = Wechater::open_in(&dir);
        wechater.settings_mut().accounts[0].is_enabled = true;
        wechater.settings_mut().accounts[0].app_id = "wx1".into();
        wechater.settings_mut().accounts[0].app_secret = "s".into();
        let error = wechater.authorize_url(None, "st").unwrap_err();
        assert!(error.to_string().contains("RedirectUri"), "{error}");

        wechater.settings_mut().accounts[0].redirect_uri = "https://e.com/cb".into();
        let url = wechater.authorize_url(None, "st").unwrap();
        assert!(url.starts_with(oauth::QR_CONNECT_URL), "{url}");
        assert!(url.contains("scope=snsapi_login"), "{url}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
