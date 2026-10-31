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
pub enum NodejsSource {
    #[default]
    Auto,
    Official,
    Npmmirror,
    Nju,
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
    pub nodejs_source: NodejsSource,
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
        let prefer_cnb = self.prefer_cnb();
        let mut ordered = urls.iter().map(String::as_str).collect::<Vec<_>>();
        ordered.sort_by_key(|url| {
            let host = reqwest::Url::parse(url)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned));
            !matches!(
                (prefer_cnb, host.as_deref()),
                (true, Some("cnb.cool"))
                    | (false, Some("github.com" | "raw.githubusercontent.com"))
            )
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
impl NodejsSource {
    pub fn urls(
        self,
        download_source: DownloadSource,
        version: &str,
        filename: &str,
    ) -> Vec<String> {
        const OFFICIAL: &str = "https://nodejs.org/dist";
        const NPMMIRROR: &str = "https://registry.npmmirror.com/-/binary/node";
        const NJU: &str = "https://mirrors.nju.edu.cn/nodejs-release";
        let bases = match self {
            Self::Auto if download_source.prefer_cnb() => [NPMMIRROR, NJU, OFFICIAL],
            Self::Auto | Self::Official => [OFFICIAL, NPMMIRROR, NJU],
            Self::Npmmirror => [NPMMIRROR, NJU, OFFICIAL],
            Self::Nju => [NJU, NPMMIRROR, OFFICIAL],
        };
        bases
            .iter()
            .map(|base| format!("{base}/v{version}/{filename}"))
            .collect()
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

    #[test]
    fn github_preference_applies_to_raw_bootstrap_downloads() {
        let urls = vec![
            "https://cnb.cool/repository/raw/master".into(),
            "https://raw.githubusercontent.com/owner/repository/master".into(),
        ];
        assert_eq!(DownloadSource::Github.order_urls(&urls)[0], urls[1]);
        assert_eq!(DownloadSource::Cnb.order_urls(&urls)[0], urls[0]);
    }

    #[test]
    fn nodejs_sources_preserve_fallbacks_and_explicit_selection_overrides_auto() {
        let version = "24.22.2";
        let filename = "node-v24.22.2-win-arm64.zip";
        let hosts = |source: NodejsSource, preference| {
            source
                .urls(preference, version, filename)
                .iter()
                .map(|url| {
                    let url = reqwest::Url::parse(url).unwrap();
                    assert_eq!(url.scheme(), "https");
                    assert!(url.path().ends_with(&format!("/v{version}/{filename}")));
                    url.host_str().unwrap().to_owned()
                })
                .collect::<Vec<_>>()
        };
        let domestic = vec!["registry.npmmirror.com", "mirrors.nju.edu.cn", "nodejs.org"];
        let official = vec!["nodejs.org", "registry.npmmirror.com", "mirrors.nju.edu.cn"];
        assert_eq!(hosts(NodejsSource::Auto, DownloadSource::Cnb), domestic);
        assert_eq!(hosts(NodejsSource::Auto, DownloadSource::Github), official);
        assert_eq!(hosts(NodejsSource::Official, DownloadSource::Cnb), official);
        assert_eq!(
            hosts(NodejsSource::Npmmirror, DownloadSource::Github),
            domestic
        );
        assert_eq!(
            hosts(NodejsSource::Nju, DownloadSource::Github),
            vec!["mirrors.nju.edu.cn", "registry.npmmirror.com", "nodejs.org"]
        );
    }
}
