//! 下载源偏好。自动规则使用系统语言与真实时区，不受 UI 语言覆盖影响。
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    #[default]
    Auto,
    Cnb,
    Github,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PythonSource {
    #[default]
    Auto,
    Nju,
    Ustc,
    Npmmirror,
    Aliyun,
    Cnb,
    Astral,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PypiSource {
    #[default]
    Auto,
    Pypi,
    Tsinghua,
    Ustc,
    Aliyun,
    Tencent,
    Huawei,
    Nju,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Preferences {
    pub download_source: DownloadSource,
    pub python_source: PythonSource,
    pub pypi_source: PypiSource,
}

fn china_environment(locale: &str, timezone: &str) -> bool {
    let locale = locale.to_ascii_lowercase().replace('_', "-");
    let simplified = locale.starts_with("zh")
        && !["hant", "-tw", "-hk", "-mo"]
            .iter()
            .any(|part| locale.contains(part));
    simplified
        && matches!(
            timezone,
            "Asia/Shanghai"
                | "Asia/Chongqing"
                | "Asia/Chungking"
                | "Asia/Harbin"
                | "PRC"
                | "China Standard Time"
        )
}

impl DownloadSource {
    pub fn prefer_cnb(self) -> bool {
        match self {
            Self::Cnb => true,
            Self::Github => false,
            Self::Auto => china_environment(
                &sys_locale::get_locale().unwrap_or_default(),
                &iana_time_zone::get_timezone().unwrap_or_default(),
            ),
        }
    }
    pub fn order_urls(self, urls: &[String]) -> Vec<&str> {
        let preferred = if self.prefer_cnb() {
            "cnb.cool"
        } else {
            "github.com"
        };
        let mut ordered = urls.iter().map(String::as_str).collect::<Vec<_>>();
        ordered.sort_by_key(|url| {
            reqwest::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .as_deref()
                != Some(preferred)
        });
        ordered
    }
}
impl PythonSource {
    pub fn mirror(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Nju => {
                Some("https://mirror.nju.edu.cn/github-release/astral-sh/python-build-standalone/")
            }
            Self::Ustc => Some(
                "https://mirrors.ustc.edu.cn/github-release/astral-sh/python-build-standalone/",
            ),
            Self::Npmmirror => {
                Some("https://registry.npmmirror.com/-/binary/python-build-standalone/")
            }
            Self::Aliyun => Some(
                "https://mirrors.aliyun.com/github/releases/astral-sh/python-build-standalone/",
            ),
            Self::Cnb => {
                Some("https://cnb.cool/astral-sh/python-build-standalone/-/releases/download/")
            }
            Self::Astral => {
                Some("https://releases.astral.sh/github/python-build-standalone/releases/download/")
            }
        }
    }
}
impl PypiSource {
    pub fn index(self, download_source: DownloadSource) -> &'static str {
        match self {
            Self::Auto if download_source.prefer_cnb() => "https://mirrors.aliyun.com/pypi/simple/",
            Self::Auto | Self::Pypi => "https://pypi.org/simple/",
            Self::Tsinghua => "https://pypi.tuna.tsinghua.edu.cn/simple/",
            Self::Ustc => "https://mirrors.ustc.edu.cn/pypi/simple/",
            Self::Aliyun => "https://mirrors.aliyun.com/pypi/simple/",
            Self::Tencent => "https://mirrors.cloud.tencent.com/pypi/simple/",
            Self::Huawei => "https://repo.huaweicloud.com/repository/pypi/simple/",
            Self::Nju => "https://mirror.nju.edu.cn/pypi/web/simple/",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_requires_simplified_chinese_and_beijing_timezone_together() {
        for locale in ["zh_CN.UTF-8", "zh-CN", "zh-Hans-CN", "zh-SG"] {
            assert!(china_environment(locale, "Asia/Shanghai"));
        }
        for (locale, timezone) in [
            ("en-US", "Asia/Shanghai"),
            ("zh-TW", "Asia/Shanghai"),
            ("zh-Hant", "Asia/Shanghai"),
            ("zh-CN", "Asia/Taipei"),
            ("zh-CN", "Asia/Singapore"),
            ("zh-CN", "America/New_York"),
        ] {
            assert!(!china_environment(locale, timezone));
        }
    }
    #[test]
    fn explicit_platform_source_reorders_without_removing_fallbacks() {
        let urls = vec!["https://cnb.cool/a".into(), "https://github.com/a".into()];
        assert_eq!(
            DownloadSource::Github.order_urls(&urls),
            vec!["https://github.com/a", "https://cnb.cool/a"]
        );
        assert_eq!(
            DownloadSource::Cnb.order_urls(&urls),
            vec!["https://cnb.cool/a", "https://github.com/a"]
        );
    }
}
