# Pek.Wechat（crate `pek-wechat`）

微信登录库（Rust）：**网站应用扫码登录 / 公众号网页授权 / 小程序登录 / 公众号全局令牌**，
接口契约参照 C# [`DH.SKIT.FlurlHttpClient.Wechat`](file:///G:/Code/Pek.FrameWork/DH.SKIT.FlurlHttpClient.Wechat)
的 `DH.SKIT.FlurlHttpClient.Wechat.Api`（`Sns*` 模型与 `WechatApiClientExecuteSnsExtensions` 调用方式）。

> 定位：**库**（非服务、无面板），供生态内其他 Rust 项目以 path 依赖复用。

## 能力一览

| 能力 | 方法 | 微信接口 | 对应 C# |
|---|---|---|---|
| 网站应用扫码登录地址 | `WechatClient::qr_connect_url` / `qr_connect_url` | `https://open.weixin.qq.com/connect/qrconnect` | `SnsOAuth2AccessToken*` 前置跳转 |
| 公众号网页授权地址 | `WechatClient::authorize_url` / `authorize_url` | `https://open.weixin.qq.com/connect/oauth2/authorize` | 同上（`snsapi_base`/`snsapi_userinfo`） |
| 回调换凭证 | `WechatClient::exchange_code` | `GET /sns/oauth2/access_token` | `ExecuteSnsOAuth2AccessTokenAsync` |
| 刷新凭证 | `WechatClient::refresh_token` | `GET /sns/oauth2/refresh_token` | `ExecuteSnsOAuth2RefreshTokenAsync` |
| 用户资料 | `WechatClient::user_info` | `GET /sns/userinfo` | `ExecuteSnsUserInfoAsync` |
| 凭证校验 | `WechatClient::check_token` | `GET /sns/auth` | `ExecuteSnsAuthAsync` |
| 一步登录 | `WechatClient::login` / `login_with_user_info` | 上面组合 | — |
| 小程序登录 | `WechatClient::js_code2session` | `GET /sns/jscode2session` | `ExecuteSnsJsCode2SessionAsync` |
| 公众号全局令牌 | `WechatClient::access_token` | `GET /cgi-bin/token`（进程内缓存 + 提前刷新） | 客户端 `AccessTokenManager` |
| 企业微信机器人推送 | `pek_wechat::wecom::WeComBot` | `cgi-bin/webhook/send` | —（复用 `dhrust::wecom`） |
| `state` 防 CSRF | `new_state` / `verify_state` | — | — |

## 快速开始

```rust
use pek_wechat::{new_state, verify_state, WechatClient};

let client = WechatClient::new("APPID", "APPSECRET");

// ① 跳转微信扫码页（state 写入会话）
let state = new_state();
let url = client.qr_connect_url("https://example.com/auth/wechat/callback", &state);

// ② 回调：校验 state，再用 code 换登录态
//    （扫码登录默认 scope=snsapi_login，可拿到昵称/头像）
if verify_state(&state, callback_state) {
    let login = client.login_with_user_info(&code)?;
    println!("openid={} unionid={:?} nickname={:?}",
        login.openid(), login.unionid(), login.nickname());
}
```

需要"跨应用"识别用户时用 `login.stable_id()`（`unionid` 优先，其次 `openid`）。

`scope=snsapi_base`（静默授权）时微信会拒绝 `/sns/userinfo`：此时 `login.user` 为 `None`、
`login.user_info_error` 记录原因，**登录本身仍然成功**（库内只告警一次）。

## 配置驱动（`Config/Wechat.toml`）

```toml
[[Accounts]]
Code = "site"
AppId = "wx..."
AppSecret = "..."
DisplayName = "网站应用扫码登录"
RedirectUri = "https://example.com/auth/wechat/callback"
Scope = "snsapi_login"   # snsapi_login / snsapi_userinfo / snsapi_base
IsDefault = true
IsEnabled = true
Order = 10
Timeout = 10000
RetryTimes = 3
InsecureTls = false
```

```rust
use pek_wechat::Wechater;

let mut wechater = Wechater::open_in(".");      // 缺失时按内嵌模板生成
for note in wechater.take_notes() { eprintln!("{note}"); }
let client = wechater.client(None)?;            // 默认账号；或 client(Some("site"))
let url = wechater.authorize_url(None, &state)?; // 回调地址取账号的 RedirectUri
```

- 读写只走 `dhrust::config::TomlConfig`：缺键带注释补齐、类型容错、损坏备份重建、
  外部编辑热加载（`wechater.reload_if_changed()`）、保存保留注释排版（`[[Accounts]]` 走 `save_with_arrays`）。
- **AppSecret 不进日志**：`WechatAccount` / `WechatClient` / `OAuthToken` / `JsCode2Session`
  的 `Debug` 一律脱敏；`session_key` 属凭据，不可下发到客户端。

## 与 DH.RustBase 的关系

- **企业微信推送早已在 DH.RustBase**：`dhrust::wecom::WeComBot`（文本 / Markdown / Markdown V2，
  本轮补齐同步入口 `send_text_blocking` / `send_markdown_blocking` / `send_value_blocking`）。
  本 crate 直接 `pub use dhrust::wecom;` 转发为 `pek_wechat::wecom`，**不复制实现**——
  一个 crate 同时覆盖"微信登录"与"企业微信推送"。
- **本轮下沉到 DH.RustBase 的公共方法**：
  - `dhrust::web::url_with_query`：URL 查询串拼接（百分号编码、片段 `#wechat_redirect` 保持在末尾）；
  - `dhrust::wecom::{send_text_blocking, send_markdown_blocking, send_value_blocking}`。
- **有意不下沉**：`WechatClient` / `Sns` 模型 / `TomlConfig` 账号选择——微信业务语义，
  只服务单一场景；令牌缓存的刷新语义属微信专属（`expires_in - 120s`）。

## 设计约束

- **同步 API**：HTTP 走 `dhrust::net::http_client::blocking_request_with_retry`
  （在 tokio 上下文内自动 offthread）；需要异步的消费方自行 `spawn_blocking`。
- **重试**：只对网络错误与 408/425/429/5xx（默认 3 次尝试，失败间隔 200ms）；
  微信业务错误（`errcode != 0`）不重试。
- **错误分类**：`WechatError::{Config, Http, Api{errcode,errmsg}, Json}`；
  `is_token_invalid()` 覆盖 40001/40014/42001/42002/42007/42009。
- **全局令牌缓存**：进程内 `Mutex` 缓存，到期时刻 = `now + expires_in - 120s`，
  并发刷新在锁内串行（单飞）；`access_token_refresh()` / `clear_access_token_cache()` 显式控制。
- **日志**：只记有意义事件（用户资料拉取失败等），不逐请求打日志。

## 目录

```
src/{lib,error,oauth,sns,client,settings,wechater}.rs
res/Wechat.toml.template        # include_str! 内嵌；改后需 touch 所属 .rs 才重编
examples/{qr_login_url,login_once}.rs
tests/sns_e2e.rs                # 本地 HTTP 桩：请求契约 + 缓存/错误分支
docs/{项目接续指南,用户档案,项目记忆,决策记录}.md
tools/continuity/*
```

## 构建 / 测试

```powershell
cargo check
cargo test              # 22 单元 + 11 端到端 + 3 文档测试
cargo clippy --all-targets
cargo run --example qr_login_url
```

- 日常写码用 `cargo check`；需要二进制时 `cargo build --profile fast`；`--release` 只用于发布。
- 依赖下载走项目内 `.cargo/config.toml`（rsproxy 镜像）。

## 与 C# 实现的差异（有意）

| 项 | C# | 本库 |
|---|---|---|
| TLS | 统一 `IgnoreSsl()` | 默认校验证书；`InsecureTls = true` 才跳过 |
| 令牌管理 | `AccessTokenManager` 可持久化 | 仅进程内缓存（提前 120s 刷新），持久化由消费方决定 |
| 登录结果 | 分两次调用 | `login_with_user_info` 合并；静默授权失败不致命 |
| 配置 | `appsettings` JSON | TOML（`Config/Wechat.toml`），可直接编辑并热加载 |
