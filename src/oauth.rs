//! 扫码登录 / 网页授权：授权地址构造与 `state` 防护。
//!
//! 对齐微信官方规范（与 C# `DH.SKIT.FlurlHttpClient.Wechat` 的 Sns 族一致）：
//!
//! | 场景 | 入口 | `scope` |
//! |---|---|---|
//! | 网站应用扫码登录 | `https://open.weixin.qq.com/connect/qrconnect` | `snsapi_login` |
//! | 公众号网页授权（取资料） | `https://open.weixin.qq.com/connect/oauth2/authorize` | `snsapi_userinfo` |
//! | 公众号网页授权（静默） | `https://open.weixin.qq.com/connect/oauth2/authorize` | `snsapi_base` |
//!
//! 授权地址必须以 `#wechat_redirect` 结尾；`redirect_uri` 必须整体 URL 编码
//! （本模块经 [`dhrust::web::url_with_query`] 自动完成）。
//!
//! `state`：随机串用于防 CSRF，调用方须在跳转前写入会话（Cookie/Session），
//! 回调时与 [`verify_state`] 比对。本项目提供 [`new_state`] 生成与常量时间比较。

use dhrust::web::url_with_query;

/// 网站应用扫码登录入口。
pub const QR_CONNECT_URL: &str = "https://open.weixin.qq.com/connect/qrconnect";

/// 公众号网页授权入口。
pub const OAUTH_AUTHORIZE_URL: &str = "https://open.weixin.qq.com/connect/oauth2/authorize";

/// 微信要求授权地址以该片段结尾。
pub const WECHAT_REDIRECT_FRAGMENT: &str = "wechat_redirect";

/// 授权范围（决定入口地址与能拿到的数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginScope {
    /// 网站应用微信登录（扫一扫；`snsapi_login`）。
    QrConnect,
    /// 公众号网页授权·静默（仅 `openid`；`snsapi_base`）。
    Base,
    /// 公众号网页授权·取用户资料（`snsapi_userinfo`）。
    UserInfo,
}

impl LoginScope {
    /// 微信协议中的 `scope` 值。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::QrConnect => "snsapi_login",
            Self::Base => "snsapi_base",
            Self::UserInfo => "snsapi_userinfo",
        }
    }

    /// 解析 `scope` 文本（未知返回 `None`；大小写不敏感）。
    pub fn from_code(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "snsapi_login" => Some(Self::QrConnect),
            "snsapi_base" => Some(Self::Base),
            "snsapi_userinfo" => Some(Self::UserInfo),
            _ => None,
        }
    }

    /// 对应的授权入口地址。
    pub fn authorize_endpoint(self) -> &'static str {
        match self {
            Self::QrConnect => QR_CONNECT_URL,
            Self::Base | Self::UserInfo => OAUTH_AUTHORIZE_URL,
        }
    }
}

/// 构造授权地址（`response_type=code`，末尾自动补 `#wechat_redirect`）。
///
/// - `redirect_uri`：回调地址（须与微信后台授权回调域一致，会被整体 URL 编码）；
/// - `state`：防 CSRF 随机串（见 [`new_state`]）。
pub fn authorize_url(
    app_id: &str,
    redirect_uri: &str,
    scope: LoginScope,
    state: &str,
) -> String {
    let mut url = url_with_query(
        scope.authorize_endpoint(),
        &[
            ("appid", app_id.trim()),
            ("redirect_uri", redirect_uri.trim()),
            ("response_type", "code"),
            ("scope", scope.as_str()),
            ("state", state),
        ],
    );
    url.push('#');
    url.push_str(WECHAT_REDIRECT_FRAGMENT);
    url
}

/// 构造网站应用扫码登录地址（`scope=snsapi_login`）。
pub fn qr_connect_url(app_id: &str, redirect_uri: &str, state: &str) -> String {
    authorize_url(app_id, redirect_uri, LoginScope::QrConnect, state)
}

/// 生成 `state`（32 位小写十六进制；16 字节 OS 熵）。
///
/// 调用方须把该值写入会话并在回调时用 [`verify_state`] 比对。
pub fn new_state() -> String {
    dhrust::random::hex(16)
}

/// 校验回调带回的 `state` 与会话中保存的值是否一致（常量时间比较）。
///
/// 任一方为空一律返回 `false`——避免"两边都没写"被误判为通过。
pub fn verify_state(expected: &str, actual: &str) -> bool {
    let (expected, actual) = (expected.as_bytes(), actual.as_bytes());
    if expected.is_empty() || expected.len() != actual.len() {
        return false;
    }
    let mut diff = 0u8;
    for (left, right) in expected.iter().zip(actual.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_connect_url_matches_wechat_spec() {
        // 官方格式：.../connect/qrconnect?appid=..&redirect_uri=..&response_type=code
        //           &scope=snsapi_login&state=..#wechat_redirect
        let url = qr_connect_url(
            "wx8888888888888888",
            "https://example.com/auth/wechat/callback",
            "abc123",
        );
        assert_eq!(
            url,
            "https://open.weixin.qq.com/connect/qrconnect?appid=wx8888888888888888&redirect_uri=https%3A%2F%2Fexample.com%2Fauth%2Fwechat%2Fcallback&response_type=code&scope=snsapi_login&state=abc123#wechat_redirect"
        );
    }

    #[test]
    fn oauth_authorize_url_switches_endpoint_by_scope() {
        let base = "https://open.weixin.qq.com/connect/oauth2/authorize";
        let user_info = authorize_url("wx1", "https://e.com/cb", LoginScope::UserInfo, "s1");
        assert!(user_info.starts_with(base), "{user_info}");
        assert!(user_info.contains("scope=snsapi_userinfo"), "{user_info}");
        assert!(user_info.ends_with("#wechat_redirect"), "{user_info}");

        let silent = authorize_url("wx1", "https://e.com/cb", LoginScope::Base, "s1");
        assert!(silent.starts_with(base), "{silent}");
        assert!(silent.contains("scope=snsapi_base"), "{silent}");
    }

    #[test]
    fn scope_round_trip() {
        assert_eq!(LoginScope::from_code("snsapi_login"), Some(LoginScope::QrConnect));
        assert_eq!(LoginScope::from_code("SNSAPI_USERINFO"), Some(LoginScope::UserInfo));
        assert_eq!(LoginScope::from_code("snsapi_base"), Some(LoginScope::Base));
        assert_eq!(LoginScope::from_code("other"), None);
        assert_eq!(LoginScope::QrConnect.as_str(), "snsapi_login");
        assert_eq!(LoginScope::QrConnect.authorize_endpoint(), QR_CONNECT_URL);
        assert_eq!(LoginScope::Base.authorize_endpoint(), OAUTH_AUTHORIZE_URL);
    }

    #[test]
    fn state_is_random_and_verified() {
        let first = new_state();
        let second = new_state();
        assert_eq!(first.len(), 32);
        assert!(first.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(first, second);

        assert!(verify_state(&first, &first));
        assert!(!verify_state(&first, &second));
        assert!(!verify_state("", ""));
        assert!(!verify_state("short", "longer"));
    }
}
