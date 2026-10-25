//! # Pek.Wechat（crate `pek-wechat`）
//!
//! 微信登录库（Rust）：**网站应用扫码登录 / 公众号网页授权 / 小程序登录 / 公众号全局令牌**，
//! 参照 C# `DH.SKIT.FlurlHttpClient.Wechat`（`DH.SKIT.FlurlHttpClient.Wechat.Api` 的 `Sns` 族）
//! 的接口契约实现；企业微信机器人推送直接复用 [`dhrust::wecom`]（本 crate 已转发为
//! [`wecom`]，无需重复引入）。
//!
//! ## 支持的能力
//!
//! | 能力 | 方法 | 微信接口 |
//! |---|---|---|
//! | 网站应用扫码登录地址 | [`WechatClient::qr_connect_url`] / [`qr_connect_url`] | `connect/qrconnect` |
//! | 公众号网页授权地址 | [`WechatClient::authorize_url`] / [`authorize_url`] | `connect/oauth2/authorize` |
//! | 回调换凭证 | [`WechatClient::exchange_code`] | `GET /sns/oauth2/access_token` |
//! | 刷新凭证 | [`WechatClient::refresh_token`] | `GET /sns/oauth2/refresh_token` |
//! | 用户资料 | [`WechatClient::user_info`] | `GET /sns/userinfo` |
//! | 凭证校验 | [`WechatClient::check_token`] | `GET /sns/auth` |
//! | 一步登录 | [`WechatClient::login`] / [`WechatClient::login_with_user_info`] | 上面组合 |
//! | 小程序登录 | [`WechatClient::js_code2session`] | `GET /sns/jscode2session` |
//! | 公众号全局令牌 | [`WechatClient::access_token`] | `GET /cgi-bin/token`（进程内缓存） |
//! | 企业微信机器人推送 | [`wecom::WeComBot`] | `cgi-bin/webhook/send` |
//!
//! ## 快速开始（扫码登录）
//!
//! ```no_run
//! use pek_wechat::{LoginScope, WechatClient, new_state, verify_state};
//!
//! let client = WechatClient::new("APPID", "APPSECRET");
//!
//! // ① 跳转微信扫码页（state 写入会话）
//! let state = new_state();
//! let url = client.qr_connect_url("https://example.com/auth/wechat/callback", &state);
//! println!("跳转：{url}");
//!
//! // ② 回调：校验 state → 用 code 换登录态（openid / unionid / 昵称 / 头像）
//! # let callback_state = String::new();
//! # let code = String::new();
//! if verify_state(&state, &callback_state) {
//!     let login = client.login_with_user_info(&code)?;
//!     println!("openid={} nickname={:?}", login.openid(), login.nickname());
//! }
//! # Ok::<(), pek_wechat::WechatError>(())
//! ```
//!
//! ## 配置驱动（`Config/Wechat.toml`）
//!
//! ```no_run
//! use pek_wechat::Wechater;
//!
//! let mut wechater = Wechater::open_in(".");
//! for note in wechater.take_notes() {
//!     dhrust::info!("{note}");
//! }
//! let client = wechater.client(None)?; // 默认账号；或 client(Some("site"))
//! # Ok::<(), pek_wechat::WechatError>(())
//! ```
//!
//! ## 设计约束
//!
//! - **同步 API**：HTTP 复用 [`dhrust::net::http_client`]（在 tokio 上下文中自动 offthread）；
//! - **不新增异步运行时**，需要异步的消费方自行 `spawn_blocking`；
//! - **配置一律 TOML**：`Config/Wechat.toml`，读写只走 [`dhrust::config::TomlConfig`]；
//! - **凭据不明文进日志**：`AppSecret` / `access_token` / `session_key` 的 `Debug` 一律脱敏；
//! - **重试**只针对网络错误与 408/425/429/5xx；微信业务错误（errcode）不重试；
//! - **不重复造轮子**：URL 查询串用 [`dhrust::web::url_with_query`]、随机 state 用
//!   [`dhrust::random::hex`]、企业微信推送用 [`dhrust::wecom`]。

pub mod client;
pub mod error;
pub mod oauth;
pub mod settings;
pub mod sns;
pub mod wechater;

/// 企业微信机器人推送（转发 [`dhrust::wecom`]，能力与实现均在 DH.RustBase）。
pub use dhrust::wecom;

pub use client::{DEFAULT_API_BASE, DEFAULT_TIMEOUT_MS, WechatClient};
pub use error::WechatError;
pub use oauth::{
    LoginScope, OAUTH_AUTHORIZE_URL, QR_CONNECT_URL, WECHAT_REDIRECT_FRAGMENT, authorize_url,
    new_state, qr_connect_url, verify_state,
};
pub use settings::{
    WECHAT_CONFIG_FILE, WECHAT_CONFIG_TEMPLATE, WechatAccount, WechatSettings, WechatSettingsFile,
};
pub use sns::{
    AccessTokenInfo, JsCode2Session, OAuthToken, WechatLogin, WechatUserInfo,
};
pub use wechater::Wechater;
