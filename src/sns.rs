//! Sns 族数据结构（对齐 C# `DH.SKIT.FlurlHttpClient.Wechat.Api` 的 `Sns*` 模型）。
//!
//! - [`OAuthToken`]：`/sns/oauth2/access_token` 与 `/sns/oauth2/refresh_token`
//! - [`WechatUserInfo`]：`/sns/userinfo`
//! - [`WechatLogin`]：一次登录的合并结果（token + 可选用户资料）
//! - [`JsCode2Session`]：`/sns/jscode2session`（小程序登录态）
//! - [`AccessTokenInfo`]：`/cgi-bin/token`（公众号全局令牌）
//!
//! 安全约定：`access_token` / `refresh_token` / `session_key` 属于凭据，
//! 相关类型的 `Debug` 一律脱敏（不以明文进入日志）。

use std::fmt;

use serde::Deserialize;

/// `GET /sns/oauth2/access_token`、`GET /sns/oauth2/refresh_token` 的响应。
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
pub struct OAuthToken {
    /// 网页授权接口调用凭证
    #[serde(default)]
    pub access_token: String,
    /// 凭证有效期（秒）
    #[serde(default)]
    pub expires_in: i64,
    /// 用户刷新访问令牌（`snsapi_base` 场景可能为空）
    #[serde(default)]
    pub refresh_token: String,
    /// 授权用户唯一标识（同一应用内唯一）
    #[serde(default)]
    pub openid: String,
    /// 用户授权的作用域（逗号分隔）
    #[serde(default)]
    pub scope: String,
    /// 开放平台唯一标识（应用已绑定开放平台时返回；跨应用识别用户用）
    #[serde(default)]
    pub unionid: Option<String>,
    /// 是否为快照页模式虚拟账号（微信「快照页」场景）
    #[serde(default)]
    pub is_snapshot_user: Option<bool>,
}

impl OAuthToken {
    /// 是否网站应用扫码登录（`scope` 含 `snsapi_login`）。
    pub fn is_qr_login(&self) -> bool {
        self.has_scope("snsapi_login")
    }

    /// 是否包含用户资料授权（`scope` 含 `snsapi_userinfo`）。
    pub fn has_user_info_scope(&self) -> bool {
        self.has_scope("snsapi_userinfo")
    }

    /// `scope` 是否包含指定值（逗号分隔；大小写不敏感）。
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scope
            .split(',')
            .any(|item| item.trim().eq_ignore_ascii_case(scope))
    }
}

impl fmt::Debug for OAuthToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OAuthToken")
            .field("access_token", &redacted(&self.access_token))
            .field("expires_in", &self.expires_in)
            .field("refresh_token", &redacted(&self.refresh_token))
            .field("openid", &self.openid)
            .field("scope", &self.scope)
            .field("unionid", &self.unionid)
            .field("is_snapshot_user", &self.is_snapshot_user)
            .finish()
    }
}

/// `GET /sns/userinfo` 的响应。
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct WechatUserInfo {
    /// 用户唯一标识
    #[serde(default)]
    pub openid: String,
    /// 开放平台唯一标识（绑定开放平台时返回）
    #[serde(default)]
    pub unionid: Option<String>,
    /// 昵称
    #[serde(default)]
    pub nickname: String,
    /// 性别：1 男，2 女，0 未知
    #[serde(default)]
    pub sex: Option<i32>,
    /// 国家
    #[serde(default)]
    pub country: Option<String>,
    /// 省份
    #[serde(default)]
    pub province: Option<String>,
    /// 城市
    #[serde(default)]
    pub city: Option<String>,
    /// 头像地址
    #[serde(default)]
    pub headimgurl: String,
    /// 用户特权信息
    #[serde(default)]
    pub privilege: Vec<String>,
}

impl WechatUserInfo {
    /// 性别中文描述（0/未知 → `None`）。
    pub fn sex_text(&self) -> Option<&'static str> {
        match self.sex {
            Some(1) => Some("男"),
            Some(2) => Some("女"),
            _ => None,
        }
    }
}

/// 一次登录（扫码 / 网页授权）的合并结果。
///
/// `user_info_error` 非空表示令牌已成功换取、仅用户资料拉取失败——
/// 典型场景：`scope=snsapi_base`（静默授权）时 `/sns/userinfo` 会被微信拒绝，
/// 此时 `user` 为 `None`，登录本身仍然成立。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WechatLogin {
    /// 网页授权凭证
    pub token: OAuthToken,
    /// 用户资料（授权含 `snsapi_userinfo` 且拉取成功时存在）
    pub user: Option<WechatUserInfo>,
    /// 用户资料拉取失败原因（登录不因此失败）
    pub user_info_error: Option<String>,
}

impl WechatLogin {
    /// 应用内用户标识（`openid`）。
    pub fn openid(&self) -> &str {
        self.user
            .as_ref()
            .map(|user| user.openid.as_str())
            .filter(|value| !value.is_empty())
            .unwrap_or(&self.token.openid)
    }

    /// 开放平台标识（优先取用户资料中的 `unionid`）。
    pub fn unionid(&self) -> Option<&str> {
        self.user
            .as_ref()
            .and_then(|user| user.unionid.as_deref())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                self.token
                    .unionid
                    .as_deref()
                    .filter(|value| !value.is_empty())
            })
    }

    /// 稳定唯一标识：`unionid` 优先，其次 `openid`（跨应用识别用户时用）。
    pub fn stable_id(&self) -> &str {
        self.unionid().unwrap_or_else(|| self.openid())
    }

    /// 昵称（未拉取用户资料时为 `None`）。
    pub fn nickname(&self) -> Option<&str> {
        self.user
            .as_ref()
            .map(|user| user.nickname.as_str())
            .filter(|value| !value.is_empty())
    }

    /// 头像地址（未拉取用户资料时为 `None`）。
    pub fn head_image_url(&self) -> Option<&str> {
        self.user
            .as_ref()
            .map(|user| user.headimgurl.as_str())
            .filter(|value| !value.is_empty())
    }

    /// 用户资料是否已成功拉取。
    pub fn has_user_info(&self) -> bool {
        self.user.is_some()
    }
}

/// `GET /sns/jscode2session` 的响应（小程序登录态）。
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
pub struct JsCode2Session {
    /// 用户唯一标识
    #[serde(default)]
    pub openid: String,
    /// 会话密钥（**凭据**，不可下发到客户端）
    #[serde(default)]
    pub session_key: String,
    /// 开放平台唯一标识
    #[serde(default)]
    pub unionid: Option<String>,
}

impl fmt::Debug for JsCode2Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsCode2Session")
            .field("openid", &self.openid)
            .field("session_key", &redacted(&self.session_key))
            .field("unionid", &self.unionid)
            .finish()
    }
}

/// `GET /cgi-bin/token` 的响应（公众号全局接口调用凭证）。
#[derive(Clone, Default, Deserialize, PartialEq, Eq)]
pub struct AccessTokenInfo {
    /// 接口调用凭证
    #[serde(default)]
    pub access_token: String,
    /// 有效期（秒；微信当前为 7200）
    #[serde(default)]
    pub expires_in: i64,
}

impl fmt::Debug for AccessTokenInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessTokenInfo")
            .field("access_token", &redacted(&self.access_token))
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

/// 凭据脱敏：非空统一显示 `***`，空串保持空串（便于区分"未配置"与"已配置"）。
fn redacted(value: &str) -> &'static str {
    if value.is_empty() { "" } else { "***" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_token_parses_wechat_payload() {
        let json = r#"{
            "access_token": "ACCESS_TOKEN",
            "expires_in": 7200,
            "refresh_token": "REFRESH_TOKEN",
            "openid": "OPENID",
            "scope": "snsapi_login,snsapi_userinfo",
            "unionid": "UNIONID",
            "is_snapshot_user": false
        }"#;
        let token: OAuthToken = serde_json::from_str(json).unwrap();
        assert_eq!(token.access_token, "ACCESS_TOKEN");
        assert_eq!(token.expires_in, 7200);
        assert_eq!(token.openid, "OPENID");
        assert_eq!(token.unionid.as_deref(), Some("UNIONID"));
        assert_eq!(token.is_snapshot_user, Some(false));
        assert!(token.is_qr_login());
        assert!(token.has_user_info_scope());
        assert!(!token.has_scope("snsapi_base"));
    }

    #[test]
    fn missing_fields_fall_back_to_default() {
        let token: OAuthToken = serde_json::from_str(r#"{"openid":"o1"}"#).unwrap();
        assert_eq!(token.openid, "o1");
        assert!(token.access_token.is_empty());
        assert!(!token.is_qr_login());
        let user: WechatUserInfo = serde_json::from_str(r#"{"openid":"o1"}"#).unwrap();
        assert_eq!(user.privilege, Vec::<String>::new());
        assert_eq!(user.sex_text(), None);
    }

    #[test]
    fn user_info_parses_and_describes_sex() {
        let json = r#"{
            "openid": "OPENID",
            "nickname": "张三",
            "sex": 1,
            "province": "湖北",
            "city": "武汉",
            "country": "中国",
            "headimgurl": "https://thirdwx.qlogo.cn/x/132",
            "privilege": ["a", "b"],
            "unionid": "UNIONID"
        }"#;
        let user: WechatUserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(user.nickname, "张三");
        assert_eq!(user.sex_text(), Some("男"));
        assert_eq!(user.privilege.len(), 2);
        assert_eq!(user.headimgurl, "https://thirdwx.qlogo.cn/x/132");
    }

    #[test]
    fn login_prefers_unionid_and_falls_back_to_openid() {
        let token = OAuthToken {
            openid: "openid-token".into(),
            unionid: Some("unionid-token".into()),
            ..OAuthToken::default()
        };
        let login = WechatLogin {
            token,
            ..WechatLogin::default()
        };
        assert_eq!(login.openid(), "openid-token");
        assert_eq!(login.unionid(), Some("unionid-token"));
        assert_eq!(login.stable_id(), "unionid-token");
        assert!(!login.has_user_info());

        let login = WechatLogin {
            token: OAuthToken {
                openid: "o1".into(),
                ..OAuthToken::default()
            },
            user: Some(WechatUserInfo {
                openid: "o1".into(),
                unionid: Some("u1".into()),
                nickname: "张三".into(),
                headimgurl: "https://img".into(),
                ..WechatUserInfo::default()
            }),
            user_info_error: None,
        };
        assert!(login.has_user_info());
        assert_eq!(login.stable_id(), "u1");
        assert_eq!(login.nickname(), Some("张三"));
        assert_eq!(login.head_image_url(), Some("https://img"));
    }

    #[test]
    fn debug_redacts_credentials() {
        let token = OAuthToken {
            access_token: "ACCESS".into(),
            refresh_token: "REFRESH".into(),
            openid: "o1".into(),
            ..OAuthToken::default()
        };
        let text = format!("{token:?}");
        assert!(!text.contains("ACCESS"), "{text}");
        assert!(!text.contains("REFRESH"), "{text}");
        assert!(text.contains("o1"), "{text}");

        let session = JsCode2Session {
            openid: "o1".into(),
            session_key: "SESSION".into(),
            unionid: None,
        };
        let text = format!("{session:?}");
        assert!(!text.contains("SESSION"), "{text}");

        let info = AccessTokenInfo {
            access_token: "GLOBAL".into(),
            expires_in: 7200,
        };
        let text = format!("{info:?}");
        assert!(!text.contains("GLOBAL"), "{text}");
        assert!(text.contains("7200"), "{text}");
    }
}
