//! 功能：限时、有界读取 GitHub Releases，区分未发布、限流与网络失败；时间：2026-11-11；作者：lq。

use serde::Deserialize;
use std::{io::Read, time::Duration};

/// 功能：关于页面与下载操作使用固定官方仓库链接；时间：2026-11-11；作者：lq。
pub const REPOSITORY_URL: &str = "https://github.com/lq0910/qds6";
pub const RELEASES_URL: &str = "https://github.com/lq0910/qds6/releases";
const LATEST_API: &str = "https://api.github.com/repos/lq0910/qds6/releases/latest";

/// 功能：UI 只展示经确认的检查结果，未发布不能声称已经最新；时间：2026-11-11；作者：lq。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateCheck {
    Current(String),
    Available(String),
    Unpublished,
}

/// 功能：仅检查正式版本，不下载或自动安装；须由后台线程调用；时间：2026-11-11；作者：lq。
pub fn check_latest(current: &str) -> Result<UpdateCheck, String> {
    check_endpoint(current, LATEST_API)
}

/// 功能：统一超时、证书验证与响应大小限制，可在回环服务验证 HTTP 行为；时间：2026-11-11；作者：lq。
fn check_endpoint(current: &str, endpoint: &str) -> Result<UpdateCheck, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!("QDS6/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| "无法初始化更新检查".to_string())?;
    let response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| "无法连接 GitHub，请检查网络后重试".to_string())?;
    match response.status().as_u16() {
        404 => return Ok(UpdateCheck::Unpublished),
        403 | 429 => return Err("GitHub 暂时限制请求，请稍后重试".into()),
        200 => {}
        status => return Err(format!("检查更新失败（HTTP {status}），请稍后重试")),
    }
    let mut body = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut body)
        .map_err(|_| "读取版本信息失败".to_string())?;
    if body.len() > 1_048_576 {
        return Err("版本信息过大，请在发布页查看".into());
    }
    parse_release(current, &body)
}

/// 功能：按语义版本比较，拒绝草稿、预发布及不可识别的标签；时间：2026-11-11；作者：lq。
fn parse_release(current: &str, body: &[u8]) -> Result<UpdateCheck, String> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
        draft: bool,
        prerelease: bool,
    }
    let release: Release =
        serde_json::from_slice(body).map_err(|_| "版本信息格式异常，请在发布页查看".to_string())?;
    let latest = semver::Version::parse(release.tag_name.trim_start_matches('v'))
        .map_err(|_| "发布标签不是有效版本号，请在发布页查看".to_string())?;
    let current = semver::Version::parse(current).map_err(|_| "当前版本号无效".to_string())?;
    if release.draft || release.prerelease || !latest.pre.is_empty() {
        return Err("未找到可用的正式版本，请在发布页查看".into());
    }
    Ok(if latest > current {
        UpdateCheck::Available(latest.to_string())
    } else {
        UpdateCheck::Current(latest.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 功能：覆盖语义版本排序、已最新、预发布和损坏响应；时间：2026-11-11；作者：lq。
    #[test]
    fn compares_semantic_versions_and_rejects_invalid_releases() {
        let release = br#"{"tag_name":"v0.10.0","draft":false,"prerelease":false}"#;
        assert_eq!(
            parse_release("0.9.0", release).unwrap(),
            UpdateCheck::Available("0.10.0".into())
        );
        assert_eq!(
            parse_release("0.10.0", release).unwrap(),
            UpdateCheck::Current("0.10.0".into())
        );
        assert!(
            parse_release(
                "0.1.0",
                br#"{"tag_name":"v0.2.0-beta.1","draft":false,"prerelease":true}"#
            )
            .is_err()
        );
        assert!(parse_release("0.1.0", b"{}").is_err());
    }

    /// 功能：真实回环 HTTP 验证未发布与限流状态，不访问外网；时间：2026-11-11；作者：lq。
    #[test]
    fn distinguishes_unpublished_and_rate_limited_http() {
        use std::{io::Write, net::TcpListener};
        for status in [404, 429] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/latest", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0; 4096];
                let _ = stream.read(&mut request);
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            });
            let result = check_endpoint("0.1.0", &endpoint);
            server.join().unwrap();
            if status == 404 {
                assert_eq!(result.unwrap(), UpdateCheck::Unpublished);
            } else {
                assert!(result.unwrap_err().contains("限制请求"));
            }
        }
    }
}
