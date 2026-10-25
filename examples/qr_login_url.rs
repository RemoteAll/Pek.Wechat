//! 示例：生成网站应用扫码登录地址（离线运行，不发网络请求）。
//!
//! ```powershell
//! cargo run --example qr_login_url
//! ```
//!
//! 把打印出的地址放进"微信登录"按钮的链接即可；用户扫码确认后微信会回调
//! `redirect_uri?code=CODE&state=STATE`（回调域名须在微信后台「授权回调域」内）。

use pek_wechat::{new_state, qr_connect_url};

fn main() {
    let app_id = std::env::var("WECHAT_APPID").unwrap_or_else(|_| "wx1234567890".to_string());
    let redirect_uri = std::env::var("WECHAT_REDIRECT_URI")
        .unwrap_or_else(|_| "https://example.com/auth/wechat/callback".to_string());

    // state 必须写入会话；回调时用 `pek_wechat::verify_state` 比对
    let state = new_state();
    let url = qr_connect_url(&app_id, &redirect_uri, &state);

    println!("state        = {state}");
    println!("扫码登录地址 = {url}");
    println!();
    println!("回调处理：校验 state → client.login_with_user_info(code)");
}
