# Pek.Wechat · 项目指令

> 全项目通用规则见用户级《跨项目接续协议》（本机 `%APPDATA%\Code\User\prompts\跨项目接续协议.instructions.md`；模板在 `tools/continuity/templates/`）。
> **开工先读（存在则读，按序）**：`.github/copilot-instructions.md` → `docs/项目接续指南.md` → `docs/用户档案.md` → `docs/项目记忆.md` → `docs/决策记录.md` 最新节。
> **机器自愈（有则快跑）**：`tools/continuity/install-global-instruction.ps1`（幂等；打开项目时 `.vscode/tasks.json` 也会自动跑）。

## 项目定位

`Pek.Wechat`（crate `pek-wechat`）是**微信登录库**（非服务、无面板），供生态内其他 Rust 项目以 path 依赖复用：
网站应用扫码登录、公众号网页授权、小程序登录、公众号全局令牌缓存（进程内）。
接口契约参照 C# `G:\Code\Pek.FrameWork\DH.SKIT.FlurlHttpClient.Wechat`
（`DH.SKIT.FlurlHttpClient.Wechat.Api` 的 `Sns*` 模型与 `WechatApiClientExecuteSnsExtensions`）。
企业微信机器人推送**直接复用** `dhrust::wecom`（本 crate 已 `pub use` 转发）。

## 依赖约定（不要重复造轮子）

| 能力 | 复用来源 | 说明 |
|---|---|---|
| 配置（TOML 模板/补齐/热加载/备份） | `dhrust::config::TomlConfig<T>`（feature `toml`） | 不自己写读写与迁移 |
| TOML 数组表写回（`[[Accounts]]`） | `TomlConfig::save_with_arrays` | 保留注释与排版 |
| HTTP 客户端（HTTPS/超时/重试/自签） | `dhrust::net::http_client`（`http-client` + `http-tls`） | 不引入新 HTTP 栈 |
| URL 查询串拼接 | `dhrust::web::url_with_query` | 片段（`#wechat_redirect`）保持在末尾 |
| 百分号编码 | `dhrust::web::url_encode` / `form_encode` | 不自己写编码 |
| JSON 信封取值 | `dhrust::json::{get_i64,get_str}` | 解析 `errcode`/`errmsg` |
| 随机 `state` | `dhrust::random::hex` | 不引入新随机源 |
| 日志 | `dhrust::logs`（`info!`/`warn_once!`） | 只记有意义事件 |
| 企业微信机器人推送 | `dhrust::wecom::WeComBot` | 文本 / Markdown / Markdown V2 + 同步入口 |

## 硬约束

- **同步 API**：库不引入异步运行时；HTTP 走 `blocking_request_with_retry`（tokio 上下文内自动 offthread）。
- **凭据不明文进日志**：`AppSecret` / `access_token` / `refresh_token` / `session_key` 的 `Debug` 一律脱敏（`***`）。
- **配置一律 TOML**（`Config/Wechat.toml`），PascalCase 键名；读写只走 `TomlConfig`。
- **重试只对网络错误与 408/425/429/5xx**；微信业务错误（`errcode != 0`）不重试。
- **静默授权容错**：`login_with_user_info` 在 `/sns/userinfo` 被拒（`snsapi_base`）时不视为登录失败。
- **可下沉判断**：新增公共能力先问"是否第二个消费方"；单一场景留在本 crate，通用件下沉 `DH.RustBase`。

## 每轮收尾（务必）

① `docs/决策记录.md` 新增一节（**含用户原话**）；② 运行 `tools/continuity/sync-memory.ps1` 同步记忆；③ 跑回归（`cargo test` + `cargo clippy --all-targets`，改动 DH.RustBase 时两仓都跑）并如实汇报；④ 提交并推送。
