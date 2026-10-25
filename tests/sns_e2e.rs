//! 端到端：本地 HTTP 桩验证扫码登录 / 网页授权 / 小程序登录 / 全局令牌缓存的请求契约。
//!
//! 桩服务按队列依次回应，并记录每次请求的请求行（方法 + 路径 + 查询串），
//! 用于断言参数拼接与顺序（对齐微信官方接口文档）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use pek_wechat::{LoginScope, WechatClient, WechatError};

/// 本地桩服务。
struct Stub {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: Option<JoinHandle<()>>,
}

impl Stub {
    /// 按给定 (状态码, 响应体) 队列启动；每次请求取一条回应。
    fn start(responses: Vec<(u16, &'static str)>) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&requests);
        let handle = std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = Vec::new();
                let mut chunk = [0u8; 1024];
                loop {
                    let read = stream.read(&mut chunk).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    buffer.extend_from_slice(&chunk[..read]);
                    if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&buffer).to_string();
                let request_line = text.lines().next().unwrap_or_default().to_string();
                captured.lock().unwrap().push(request_line);

                let response = format!(
                    "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        Self {
            base_url: format!("http://127.0.0.1:{port}"),
            requests,
            handle: Some(handle),
        }
    }

    fn client(&self) -> WechatClient {
        WechatClient::new("wx-app", "wx-secret")
            .with_base_url(&self.base_url)
            .with_attempts(1)
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        // 桩线程会在连接读完后自然退出；显式连接一次让它在断言失败时也能收尾
        let _ = std::net::TcpStream::connect(self.base_url.replace("http://", ""));
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

const TOKEN_JSON: &str = r#"{
    "access_token": "ACCESS-TOKEN",
    "expires_in": 7200,
    "refresh_token": "REFRESH-TOKEN",
    "openid": "OPENID-1",
    "scope": "snsapi_login,snsapi_userinfo",
    "unionid": "UNIONID-1",
    "is_snapshot_user": false
}"#;

const USER_JSON: &str = r#"{
    "openid": "OPENID-1",
    "nickname": "张三",
    "sex": 1,
    "province": "湖北",
    "city": "武汉",
    "country": "中国",
    "headimgurl": "https://thirdwx.qlogo.cn/x/132",
    "privilege": [],
    "unionid": "UNIONID-1"
}"#;

#[test]
fn exchange_code_matches_wechat_contract() {
    let stub = Stub::start(vec![(200, TOKEN_JSON)]);
    let token = stub.client().exchange_code("CODE-1").unwrap();

    assert_eq!(token.access_token, "ACCESS-TOKEN");
    assert_eq!(token.refresh_token, "REFRESH-TOKEN");
    assert_eq!(token.openid, "OPENID-1");
    assert_eq!(token.unionid.as_deref(), Some("UNIONID-1"));
    assert!(token.is_qr_login());
    assert!(token.has_user_info_scope());

    let requests = stub.requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].starts_with("GET /sns/oauth2/access_token?"), "{requests:?}");
    assert!(
        requests[0].contains("appid=wx-app&secret=wx-secret&code=CODE-1&grant_type=authorization_code"),
        "{requests:?}"
    );
}

#[test]
fn login_merges_token_and_user_info() {
    let stub = Stub::start(vec![(200, TOKEN_JSON), (200, USER_JSON)]);
    let login = stub.client().login_with_user_info("CODE-1").unwrap();

    assert!(login.has_user_info());
    assert_eq!(login.openid(), "OPENID-1");
    assert_eq!(login.stable_id(), "UNIONID-1");
    assert_eq!(login.nickname(), Some("张三"));
    assert_eq!(login.head_image_url(), Some("https://thirdwx.qlogo.cn/x/132"));
    assert_eq!(login.user_info_error, None);
    assert_eq!(login.user.as_ref().unwrap().sex_text(), Some("男"));

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(requests[1].starts_with("GET /sns/userinfo?"), "{requests:?}");
    assert!(
        requests[1].contains("access_token=ACCESS-TOKEN&openid=OPENID-1&lang=zh_CN"),
        "{requests:?}"
    );
}

#[test]
fn silent_scope_keeps_login_when_user_info_is_rejected() {
    // snsapi_base 静默授权：/sns/userinfo 返回 48001（api 未授权）→ 登录仍成立
    let stub = Stub::start(vec![
        (200, TOKEN_JSON),
        (200, r#"{"errcode":48001,"errmsg":"api unauthorized"}"#),
    ]);
    let login = stub.client().login_with_user_info("CODE-1").unwrap();
    assert!(!login.has_user_info());
    assert_eq!(login.stable_id(), "UNIONID-1");
    assert_eq!(login.openid(), "OPENID-1");
    let reason = login.user_info_error.expect("应记录用户资料失败原因");
    assert!(reason.contains("48001"), "{reason}");
}

#[test]
fn refresh_token_and_check_token_hit_expected_endpoints() {
    let stub = Stub::start(vec![
        (200, TOKEN_JSON),
        (200, r#"{"errcode":0,"errmsg":"ok"}"#),
    ]);
    let client = stub.client();
    let token = client.refresh_token("REFRESH-TOKEN").unwrap();
    assert_eq!(token.access_token, "ACCESS-TOKEN");
    client.check_token("ACCESS-TOKEN", "OPENID-1").unwrap();

    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(
        requests[0].starts_with("GET /sns/oauth2/refresh_token?appid=wx-app&grant_type=refresh_token&refresh_token=REFRESH-TOKEN"),
        "{requests:?}"
    );
    assert!(
        requests[1].starts_with("GET /sns/auth?access_token=ACCESS-TOKEN&openid=OPENID-1"),
        "{requests:?}"
    );
}

#[test]
fn check_token_surfaces_invalid_token_error() {
    let stub = Stub::start(vec![(200, r#"{"errcode":40003,"errmsg":"invalid openid"}"#)]);
    let error = stub.client().check_token("T", "o1").unwrap_err();
    assert_eq!(error.errcode(), Some(40003));
    assert!(!error.is_token_invalid(), "40003 不是令牌失效码：{error}");

    let stub = Stub::start(vec![(200, r#"{"errcode":40001,"errmsg":"invalid credential"}"#)]);
    let error = stub.client().check_token("T", "o1").unwrap_err();
    assert!(error.is_token_invalid(), "{error}");
}

#[test]
fn login_without_user_info_only_calls_one_endpoint() {
    let stub = Stub::start(vec![(200, TOKEN_JSON)]);
    let login = stub.client().login("CODE-1").unwrap();
    assert_eq!(login.openid(), "OPENID-1");
    assert_eq!(login.nickname(), None);
    assert_eq!(stub.requests().len(), 1);
}

#[test]
fn js_code2session_matches_wechat_contract() {
    let stub = Stub::start(vec![(
        200,
        r#"{"openid":"OPENID-2","session_key":"SESSION-KEY","unionid":"UNIONID-2"}"#,
    )]);
    let session = stub.client().js_code2session("JS-CODE").unwrap();
    assert_eq!(session.openid, "OPENID-2");
    assert_eq!(session.session_key, "SESSION-KEY");
    assert_eq!(session.unionid.as_deref(), Some("UNIONID-2"));
    // 会话密钥不得出现在 Debug 输出中
    assert!(!format!("{session:?}").contains("SESSION-KEY"));

    let requests = stub.requests();
    assert!(
        requests[0].starts_with(
            "GET /sns/jscode2session?appid=wx-app&secret=wx-secret&js_code=JS-CODE&grant_type=authorization_code"
        ),
        "{requests:?}"
    );
}

#[test]
fn global_access_token_is_cached_between_calls() {
    let stub = Stub::start(vec![(200, r#"{"access_token":"GLOBAL-TOKEN","expires_in":7200}"#)]);
    let client = stub.client();

    assert_eq!(client.access_token().unwrap(), "GLOBAL-TOKEN");
    assert_eq!(client.cached_access_token().as_deref(), Some("GLOBAL-TOKEN"));
    // 第二次命中缓存：不再发起请求（桩只准备了一条回应，多请求会阻塞）
    assert_eq!(client.access_token().unwrap(), "GLOBAL-TOKEN");
    assert_eq!(stub.requests().len(), 1);

    client.clear_access_token_cache();
    assert!(client.cached_access_token().is_none());
}

#[test]
fn access_token_uses_client_credential_grant() {
    let stub = Stub::start(vec![(200, r#"{"access_token":"GLOBAL-TOKEN","expires_in":7200}"#)]);
    let _ = stub.client().access_token().unwrap();
    let requests = stub.requests();
    assert!(
        requests[0].starts_with(
            "GET /cgi-bin/token?grant_type=client_credential&appid=wx-app&secret=wx-secret"
        ),
        "{requests:?}"
    );
}

#[test]
fn http_failure_and_bad_json_are_reported() {
    let stub = Stub::start(vec![(500, "internal error")]);
    let error = stub.client().exchange_code("CODE-1").unwrap_err();
    assert!(matches!(error, WechatError::Http(_)), "{error}");
    assert!(error.to_string().contains("500"), "{error}");

    let stub = Stub::start(vec![(200, "<html>not json</html>")]);
    let error = stub.client().exchange_code("CODE-1").unwrap_err();
    assert!(matches!(error, WechatError::Json(_)), "{error}");
}

#[test]
fn authorize_urls_use_configured_appid() {
    let client = WechatClient::new("wx-app", "wx-secret");
    let state = pek_wechat::new_state();
    let url = client.qr_connect_url("https://example.com/auth/wechat/callback", &state);
    assert!(url.contains("appid=wx-app"), "{url}");
    assert!(url.contains("scope=snsapi_login"), "{url}");
    assert!(url.ends_with("#wechat_redirect"), "{url}");

    let url = client.authorize_url("https://example.com/cb", LoginScope::UserInfo, &state);
    assert!(url.contains("connect/oauth2/authorize"), "{url}");
    assert!(url.contains("scope=snsapi_userinfo"), "{url}");
}
