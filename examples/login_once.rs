//! 示例：用回调 `code` 完成一次登录（真实调用微信接口）。
//!
//! ```powershell
//! $env:WECHAT_APPID="wx..."          # 网站应用 / 公众号 AppId
//! $env:WECHAT_SECRET="..."           # 对应 AppSecret
//! cargo run --example login_once -- <回调带回的 code>
//! ```
//!
//! 输出 openid / unionid / 昵称 / 头像；`scope=snsapi_base` 时昵称与头像为空
//! （微信拒绝 `/sns/userinfo`，属于预期行为）。

use std::process::ExitCode;

use pek_wechat::WechatClient;

fn main() -> ExitCode {
    let code = match std::env::args().nth(1) {
        Some(code) if !code.trim().is_empty() => code,
        _ => {
            eprintln!("用法：cargo run --example login_once -- <code>");
            return ExitCode::from(2);
        }
    };
    let app_id = std::env::var("WECHAT_APPID").unwrap_or_default();
    let app_secret = std::env::var("WECHAT_SECRET").unwrap_or_default();
    if app_id.is_empty() || app_secret.is_empty() {
        eprintln!("请先设置环境变量 WECHAT_APPID / WECHAT_SECRET");
        return ExitCode::from(2);
    }

    let client = WechatClient::new(app_id, app_secret);
    match client.login_with_user_info(&code) {
        Ok(login) => {
            println!("openid      = {}", login.openid());
            println!("unionid     = {:?}", login.unionid());
            println!("stable_id   = {}", login.stable_id());
            println!("nickname    = {:?}", login.nickname());
            println!("headimgurl  = {:?}", login.head_image_url());
            println!("scope       = {}", login.token.scope);
            if let Some(reason) = &login.user_info_error {
                println!("user_info   = 拉取失败（登录仍有效）：{reason}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("登录失败：{error}");
            ExitCode::FAILURE
        }
    }
}
