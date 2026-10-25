//! 微信客户端（单账号）：扫码登录 / 网页授权 / 小程序登录 / 公众号全局令牌。
//!
//! 设计约定（与 `Pek.RSms` / `Pek.RMail` 一致）：
//!
//! - **同步 API**：HTTP 走 [`dhrust::net::http_client::blocking_request_auto_with`]，
//!   在 tokio 上下文内会自动切到独立线程（不会触发"运行时内 block_on"）；
//! - **不新增异步运行时**：需要异步的消费方自行 `spawn_blocking`；
//! - **重试**：仅对网络错误与 408/425/429/5xx 重试，业务错误（errcode）不重试；
//! - **令牌缓存**：仅公众号全局 `access_token`（`/cgi-bin/token`）在进程内缓存并
//!   提前刷新；网页授权凭证属"每次登录一份"，由调用方随会话保存。

use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use dhrust::net::http_client::{self, HttpClientOptions, HttpResponse, RetryPolicy};

use crate::error::WechatError;
use crate::oauth::{self, LoginScope};
use crate::settings::WechatAccount;
use crate::sns::{AccessTokenInfo, JsCode2Session, OAuthToken, WechatLogin, WechatUserInfo};

/// 微信 API 默认基地址。
pub const DEFAULT_API_BASE: &str = "https://api.weixin.qq.com";

/// 默认请求超时（毫秒）。
pub const DEFAULT_TIMEOUT_MS: u64 = 10_000;

/// 全局令牌提前刷新量（秒）：距过期不足该值时视为需要刷新。
const TOKEN_REFRESH_AHEAD_SECS: u64 = 120;

/// 进程内全局令牌缓存项。
struct CachedToken {
    token: String,
    /// 到期时刻（已扣除提前刷新量）
    expires_at: Instant,
}

/// 微信客户端（单个应用账号）。
///
/// 创建开销极小（仅字符串 + 一个空缓存），可按请求创建；同一实例内
/// [`WechatClient::access_token`] 会复用进程内缓存的全局令牌。
pub struct WechatClient {
    app_id: String,
    app_secret: String,
    base_url: String,
    timeout: Duration,
    attempts: u32,
    insecure_tls: bool,
    token_cache: Mutex<Option<CachedToken>>,
}

impl fmt::Debug for WechatClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WechatClient")
            .field("app_id", &self.app_id)
            .field("app_secret", &if self.app_secret.is_empty() { "" } else { "***" })
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("attempts", &self.attempts)
            .field("insecure_tls", &self.insecure_tls)
            .finish()
    }
}

impl WechatClient {
    /// 创建客户端（AppId/AppSecret 必填，缺失在调用接口时报错）。
    pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into().trim().to_string(),
            app_secret: app_secret.into().trim().to_string(),
            base_url: DEFAULT_API_BASE.to_string(),
            timeout: Duration::from_millis(DEFAULT_TIMEOUT_MS),
            attempts: 1,
            insecure_tls: false,
            token_cache: Mutex::new(None),
        }
    }

    /// 按配置账号创建客户端（超时/重试/自签开关取自账号配置）。
    pub fn from_account(account: &WechatAccount) -> Self {
        Self::new(account.app_id.clone(), account.app_secret.clone())
            .with_timeout_ms(account.timeout_ms())
            .with_attempts(account.attempts())
            .with_insecure_tls(account.insecure_tls)
    }

    /// AppId。
    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    /// 覆盖 API 基地址（默认 [`DEFAULT_API_BASE`]；本地联调/代理时使用）。
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        let base = base_url.into();
        let trimmed = base.trim().trim_end_matches('/');
        self.base_url = if trimmed.is_empty() {
            DEFAULT_API_BASE.to_string()
        } else {
            trimmed.to_string()
        };
        self
    }

    /// 设置请求超时。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 设置请求超时（毫秒；0 按默认 10000）。
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        let ms = if timeout_ms == 0 {
            DEFAULT_TIMEOUT_MS
        } else {
            timeout_ms
        };
        self.timeout = Duration::from_millis(ms.max(1000));
        self
    }

    /// 设置总尝试次数（`<=1` 表示不重试）。
    pub fn with_attempts(mut self, attempts: u32) -> Self {
        self.attempts = attempts.max(1);
        self
    }

    /// 设置是否跳过 TLS 证书校验（自签/内网代理调试；默认校验证书）。
    pub fn with_insecure_tls(mut self, insecure: bool) -> Self {
        self.insecure_tls = insecure;
        self
    }

    // ---------------------------------------------------------------- 授权地址

    /// 构造授权地址（网站应用扫码登录 / 公众号网页授权）。
    pub fn authorize_url(&self, redirect_uri: &str, scope: LoginScope, state: &str) -> String {
        oauth::authorize_url(&self.app_id, redirect_uri, scope, state)
    }

    /// 构造网站应用扫码登录地址（`scope=snsapi_login`）。
    pub fn qr_connect_url(&self, redirect_uri: &str, state: &str) -> String {
        oauth::qr_connect_url(&self.app_id, redirect_uri, state)
    }

    // ---------------------------------------------------------- 扫码登录 / 授权

    /// 用回调 `code` 换取网页授权凭证（`GET /sns/oauth2/access_token`）。
    pub fn exchange_code(&self, code: &str) -> Result<OAuthToken, WechatError> {
        self.ensure_credentials()?;
        let code = code.trim();
        if code.is_empty() {
            return Err(WechatError::config("授权 code 为空"));
        }
        let value = self.get_json(
            "/sns/oauth2/access_token",
            &[
                ("appid", self.app_id.as_str()),
                ("secret", self.app_secret.as_str()),
                ("code", code),
                ("grant_type", "authorization_code"),
            ],
        )?;
        decode(&value)
    }

    /// 刷新网页授权凭证（`GET /sns/oauth2/refresh_token`）。
    pub fn refresh_token(&self, refresh_token: &str) -> Result<OAuthToken, WechatError> {
        self.ensure_credentials()?;
        let refresh_token = refresh_token.trim();
        if refresh_token.is_empty() {
            return Err(WechatError::config("refresh_token 为空"));
        }
        let value = self.get_json(
            "/sns/oauth2/refresh_token",
            &[
                ("appid", self.app_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ],
        )?;
        decode(&value)
    }

    /// 拉取用户资料（`GET /sns/userinfo`；`lang` 传空按 `zh_CN`）。
    pub fn user_info(
        &self,
        access_token: &str,
        openid: &str,
        lang: &str,
    ) -> Result<WechatUserInfo, WechatError> {
        let access_token = access_token.trim();
        let openid = openid.trim();
        if access_token.is_empty() {
            return Err(WechatError::config("access_token 为空"));
        }
        if openid.is_empty() {
            return Err(WechatError::config("openid 为空"));
        }
        let lang = if lang.trim().is_empty() {
            "zh_CN"
        } else {
            lang.trim()
        };
        let value = self.get_json(
            "/sns/userinfo",
            &[
                ("access_token", access_token),
                ("openid", openid),
                ("lang", lang),
            ],
        )?;
        decode(&value)
    }

    /// 校验网页授权凭证是否有效（`GET /sns/auth`；`errcode=0` 为有效）。
    pub fn check_token(&self, access_token: &str, openid: &str) -> Result<(), WechatError> {
        let access_token = access_token.trim();
        let openid = openid.trim();
        if access_token.is_empty() {
            return Err(WechatError::config("access_token 为空"));
        }
        if openid.is_empty() {
            return Err(WechatError::config("openid 为空"));
        }
        self.get_json(
            "/sns/auth",
            &[("access_token", access_token), ("openid", openid)],
        )?;
        Ok(())
    }

    /// 扫码/网页授权回调：换取凭证（不含用户资料）。
    pub fn login(&self, code: &str) -> Result<WechatLogin, WechatError> {
        let token = self.exchange_code(code)?;
        Ok(WechatLogin {
            token,
            ..WechatLogin::default()
        })
    }

    /// 扫码/网页授权回调：换取凭证并尽力拉取用户资料。
    ///
    /// `scope=snsapi_base`（静默授权）时 `/sns/userinfo` 会被微信拒绝，
    /// 此时返回的 [`WechatLogin::user`] 为 `None`、[`WechatLogin::user_info_error`]
    /// 记录原因，**登录本身仍然成功**（只告警一次，不打断流程）。
    pub fn login_with_user_info(&self, code: &str) -> Result<WechatLogin, WechatError> {
        let token = self.exchange_code(code)?;
        let mut result = WechatLogin {
            token,
            ..WechatLogin::default()
        };
        match self.user_info(&result.token.access_token, &result.token.openid, "zh_CN") {
            Ok(user) => result.user = Some(user),
            Err(error) => {
                dhrust::warn_once!(
                    "微信用户资料拉取失败（登录继续，openid={}）：{error}",
                    result.token.openid
                );
                result.user_info_error = Some(error.to_string());
            }
        }
        Ok(result)
    }

    // ------------------------------------------------------------------ 小程序

    /// 小程序登录：用 `wx.login` 的 code 换取 `openid` / `session_key`
    /// （`GET /sns/jscode2session`）。
    pub fn js_code2session(&self, js_code: &str) -> Result<JsCode2Session, WechatError> {
        self.ensure_credentials()?;
        let js_code = js_code.trim();
        if js_code.is_empty() {
            return Err(WechatError::config("js_code 为空"));
        }
        let value = self.get_json(
            "/sns/jscode2session",
            &[
                ("appid", self.app_id.as_str()),
                ("secret", self.app_secret.as_str()),
                ("js_code", js_code),
                ("grant_type", "authorization_code"),
            ],
        )?;
        decode(&value)
    }

    // --------------------------------------------------------- 公众号全局令牌

    /// 取公众号全局 `access_token`（`GET /cgi-bin/token`，进程内缓存）。
    ///
    /// 缓存策略：有效期减 120 秒作为到期时刻；到期或未命中时重新获取
    /// （并发调用在锁内串行，天然单飞，不会出现"同时刷新多份"）。
    pub fn access_token(&self) -> Result<String, WechatError> {
        self.access_token_info().map(|info| info.access_token)
    }

    /// 取全局令牌及其有效期（命中缓存时返回剩余口径的原始 `expires_in`）。
    pub fn access_token_info(&self) -> Result<AccessTokenInfo, WechatError> {
        self.ensure_credentials()?;
        let mut cache = self
            .token_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.as_ref()
            && Instant::now() < cached.expires_at
        {
            return Ok(AccessTokenInfo {
                access_token: cached.token.clone(),
                expires_in: cached.expires_at.saturating_duration_since(Instant::now()).as_secs()
                    as i64,
            });
        }
        let info: AccessTokenInfo = self.fetch_access_token()?;
        let ttl = info
            .expires_in
            .max(0)
            .unsigned_abs()
            .saturating_sub(TOKEN_REFRESH_AHEAD_SECS);
        *cache = Some(CachedToken {
            token: info.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl),
        });
        Ok(info)
    }

    /// 强制刷新全局令牌（忽略缓存；如遇 40001/42001 等失效码时调用）。
    pub fn access_token_refresh(&self) -> Result<String, WechatError> {
        self.ensure_credentials()?;
        let info: AccessTokenInfo = self.fetch_access_token()?;
        let ttl = info
            .expires_in
            .max(0)
            .unsigned_abs()
            .saturating_sub(TOKEN_REFRESH_AHEAD_SECS);
        let mut cache = self
            .token_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = Some(CachedToken {
            token: info.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(ttl),
        });
        Ok(info.access_token)
    }

    /// 清空全局令牌缓存。
    pub fn clear_access_token_cache(&self) {
        let mut cache = self
            .token_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *cache = None;
    }

    /// 当前缓存的全局令牌（不做网络请求；未缓存/已到期返回 `None`）。
    pub fn cached_access_token(&self) -> Option<String> {
        let cache = self
            .token_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache
            .as_ref()
            .filter(|cached| Instant::now() < cached.expires_at)
            .map(|cached| cached.token.clone())
    }

    fn fetch_access_token(&self) -> Result<AccessTokenInfo, WechatError> {
        let value = self.get_json(
            "/cgi-bin/token",
            &[
                ("grant_type", "client_credential"),
                ("appid", self.app_id.as_str()),
                ("secret", self.app_secret.as_str()),
            ],
        )?;
        decode(&value)
    }

    // ------------------------------------------------------------------ 内部

    /// 凭据校验（AppId/AppSecret 为空时给出可操作提示）。
    fn ensure_credentials(&self) -> Result<(), WechatError> {
        if self.app_id.is_empty() {
            return Err(WechatError::config("AppId 未配置"));
        }
        if self.app_secret.is_empty() {
            return Err(WechatError::config("AppSecret 未配置"));
        }
        Ok(())
    }

    /// GET + JSON 信封校验（非 0 `errcode` 转 [`WechatError::Api`]）。
    fn get_json(
        &self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<serde_json::Value, WechatError> {
        let url = dhrust::web::url_with_query(
            &format!("{}{path}", self.base_url.trim_end_matches('/')),
            query,
        );
        let options = HttpClientOptions {
            timeout: self.timeout,
            insecure_tls: self.insecure_tls,
            ca_pem: None,
        };
        let policy = RetryPolicy::new(self.attempts)
            .with_delay(Duration::from_millis(200))
            .with_statuses([408, 425, 429, 500, 502, 503, 504]);
        let response = http_client::blocking_request_with_retry(
            "GET",
            &url,
            &[],
            None,
            Vec::new(),
            options,
            &policy,
        )
        .map_err(|error| WechatError::Http(error.to_string()))?;
        parse_response(response)
    }
}

/// 校验响应并解析为 JSON（HTTP 非 2xx → 网络错误；非 0 errcode → 业务错误）。
fn parse_response(response: HttpResponse) -> Result<serde_json::Value, WechatError> {
    let status = response.status;
    let body = response.body_text();
    if !response.is_success() {
        return Err(WechatError::Http(format!("HTTP {status}：{body}")));
    }
    let value: serde_json::Value = serde_json::from_str(body.trim())
        .map_err(|error| WechatError::json(format!("响应不是合法 JSON：{error}；原文：{body}")))?;
    if let Some(errcode) = dhrust::json::get_i64(&value, "errcode")
        && errcode != 0
    {
        let errmsg = dhrust::json::get_str(&value, "errmsg").unwrap_or_default();
        return Err(WechatError::api(errcode, errmsg));
    }
    Ok(value)
}

/// JSON → 模型（字段缺失按 `#[serde(default)]` 兜底，类型不符报错误）。
fn decode<T>(value: &serde_json::Value) -> Result<T, WechatError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    T::deserialize(value)
        .map_err(|error| WechatError::json(format!("响应字段解析失败：{error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_credentials_are_rejected_before_network() {
        let client = WechatClient::new("", "");
        let error = client.exchange_code("code").unwrap_err();
        assert!(matches!(error, WechatError::Config(_)), "{error}");
        assert!(error.to_string().contains("AppId 未配置"), "{error}");

        let client = WechatClient::new("wx1", "");
        let error = client.js_code2session("code").unwrap_err();
        assert!(error.to_string().contains("AppSecret 未配置"), "{error}");
    }

    #[test]
    fn empty_arguments_are_rejected_locally() {
        let client = WechatClient::new("wx1", "secret");
        assert!(matches!(
            client.exchange_code("  ").unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(matches!(
            client.refresh_token("").unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(matches!(
            client.user_info("", "o1", "zh_CN").unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(matches!(
            client.user_info("token", " ", "zh_CN").unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(matches!(
            client.check_token("token", "").unwrap_err(),
            WechatError::Config(_)
        ));
        assert!(matches!(
            client.js_code2session("").unwrap_err(),
            WechatError::Config(_)
        ));
    }

    #[test]
    fn response_envelope_reports_wechat_error() {
        let response = fake_response(200, r#"{"errcode":40029,"errmsg":"invalid code"}"#);
        let error = parse_response(response).unwrap_err();
        assert_eq!(error.errcode(), Some(40029));
        assert!(error.to_string().contains("invalid code"), "{error}");

        let response = fake_response(200, r#"{"errcode":0,"errmsg":"ok"}"#);
        assert!(parse_response(response).is_ok());

        let response = fake_response(200, r#"{"access_token":"A"}"#);
        assert!(parse_response(response).is_ok());

        let response = fake_response(502, "bad gateway");
        let error = parse_response(response).unwrap_err();
        assert!(matches!(error, WechatError::Http(_)), "{error}");
        assert!(error.to_string().contains("502"), "{error}");

        let response = fake_response(200, "not-json");
        let error = parse_response(response).unwrap_err();
        assert!(matches!(error, WechatError::Json(_)), "{error}");
    }

    #[test]
    fn debug_redacts_app_secret() {
        let client = WechatClient::new("wx1", "top-secret");
        let text = format!("{client:?}");
        assert!(!text.contains("top-secret"), "{text}");
        assert!(text.contains("wx1"), "{text}");
    }

    #[test]
    fn base_url_override_is_trimmed() {
        let client = WechatClient::new("wx1", "s").with_base_url("http://127.0.0.1:1/");
        let url = dhrust::web::url_with_query(
            &format!("{}/sns/auth", client.base_url.trim_end_matches('/')),
            &[("access_token", "t")],
        );
        assert_eq!(url, "http://127.0.0.1:1/sns/auth?access_token=t");
    }

    /// 构造测试用响应（`HttpResponse` 字段与真实响应同构）。
    fn fake_response(status: u16, body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        }
    }
}
