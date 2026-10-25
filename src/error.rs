//! 错误类型（统一承载配置、网络、业务与解析四类失败）。

use std::fmt;

/// 微信接口错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WechatError {
    /// 配置缺失/非法（账号未配置、AppId 为空、code 为空等）。
    Config(String),
    /// 网络/HTTP 层失败（连接、超时、非 2xx）。
    Http(String),
    /// 微信业务错误（响应含非 0 `errcode`）。
    Api {
        /// 微信错误码
        errcode: i64,
        /// 微信错误描述
        errmsg: String,
    },
    /// 响应 JSON 解析失败（非 JSON 或字段类型不符）。
    Json(String),
}

impl WechatError {
    /// 配置错误。
    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    /// 网络/HTTP 错误。
    pub fn http(message: impl Into<String>) -> Self {
        Self::Http(message.into())
    }

    /// JSON 解析错误。
    pub fn json(message: impl Into<String>) -> Self {
        Self::Json(message.into())
    }

    /// 微信业务错误。
    pub fn api(errcode: i64, errmsg: impl Into<String>) -> Self {
        Self::Api {
            errcode,
            errmsg: errmsg.into(),
        }
    }

    /// 微信业务错误码（非 [`WechatError::Api`] 返回 `None`）。
    pub fn errcode(&self) -> Option<i64> {
        match self {
            Self::Api { errcode, .. } => Some(*errcode),
            _ => None,
        }
    }

    /// 是否属于「令牌无效/过期」类错误。
    ///
    /// 覆盖公众号 `access_token` 与网页授权 `access_token` 的常见失效码：
    /// 40001（凭证无效）、40014（不合法 token）、42001（token 超时）、
    /// 42002（refresh_token 超时）、42007（用户修改密码）、42009（凭证过期）。
    pub fn is_token_invalid(&self) -> bool {
        matches!(
            self.errcode(),
            Some(40001 | 40014 | 42001 | 42002 | 42007 | 42009)
        )
    }
}

impl fmt::Display for WechatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => write!(f, "微信配置错误：{message}"),
            Self::Http(message) => write!(f, "微信网络请求失败：{message}"),
            Self::Api { errcode, errmsg } => {
                write!(f, "微信接口返回错误（errcode={errcode}）：{errmsg}")
            }
            Self::Json(message) => write!(f, "微信响应解析失败：{message}"),
        }
    }
}

impl std::error::Error for WechatError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_carries_actionable_text() {
        assert!(WechatError::config("AppId 为空").to_string().contains("AppId 为空"));
        let api = WechatError::api(40029, "invalid code");
        assert!(api.to_string().contains("40029"), "{api}");
        assert!(api.to_string().contains("invalid code"), "{api}");
        assert_eq!(api.errcode(), Some(40029));
        assert!(WechatError::http("timeout").to_string().contains("timeout"));
        assert!(WechatError::json("bad").to_string().contains("bad"));
    }

    #[test]
    fn token_invalid_codes_are_recognized() {
        for code in [40001, 40014, 42001, 42002, 42007, 42009] {
            assert!(WechatError::api(code, "x").is_token_invalid(), "{code}");
        }
        assert!(!WechatError::api(40029, "invalid code").is_token_invalid());
        assert!(!WechatError::http("timeout").is_token_invalid());
    }
}
