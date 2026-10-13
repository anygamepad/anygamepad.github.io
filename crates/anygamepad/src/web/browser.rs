//! Browser engine and OS detection from the user agent string.

use anygamepad_model::identity::{Browser, BrowserEngine, Os};

/// The browser engine, its major version and the OS, from
/// `navigator.userAgent`.
///
/// `max_touch_points` is `navigator.maxTouchPoints`: Safari on iPadOS
/// presents itself as macOS, and touch support tells them apart.
///
/// The version is the engine's: Chromium's for every Blink browser (Chrome,
/// Edge, Opera, ...), Firefox's for Gecko, Safari's for WebKit. On iOS every
/// browser uses WebKit, so the version is Safari's (the iOS version when the
/// user agent does not say). ChromeOS counts as Linux, since Chromium reads
/// gamepads there the way it does on Linux.
pub fn detect(user_agent: &str, max_touch_points: u32) -> (Browser, Os) {
    let os = detect_os(user_agent, max_touch_points);
    let (engine, major_version) = if os == Os::Ios {
        let version = version_after(user_agent, "Version/")
            .or_else(|| version_after(user_agent, " OS ").filter(|_| has_ios_device(user_agent)));
        (BrowserEngine::WebKit, version)
    } else if user_agent.contains("Firefox/") {
        (BrowserEngine::Gecko, version_after(user_agent, "Firefox/"))
    } else if user_agent.contains("Chrome/") {
        (BrowserEngine::Blink, version_after(user_agent, "Chrome/"))
    } else if user_agent.contains("Chromium/") {
        (BrowserEngine::Blink, version_after(user_agent, "Chromium/"))
    } else if user_agent.contains("AppleWebKit/") {
        (BrowserEngine::WebKit, version_after(user_agent, "Version/"))
    } else {
        (BrowserEngine::Unknown, None)
    };
    let browser = Browser {
        engine,
        major_version,
    };
    (browser, os)
}

fn detect_os(user_agent: &str, max_touch_points: u32) -> Os {
    if user_agent.contains("Android") {
        Os::Android
    } else if has_ios_device(user_agent) {
        Os::Ios
    } else if user_agent.contains("Windows") {
        Os::Windows
    } else if user_agent.contains("Macintosh") || user_agent.contains("Mac OS X") {
        if max_touch_points > 1 {
            Os::Ios
        } else {
            Os::MacOs
        }
    } else if user_agent.contains("CrOS") || user_agent.contains("Linux") {
        Os::Linux
    } else {
        Os::Unknown
    }
}

fn has_ios_device(user_agent: &str) -> bool {
    ["iPhone", "iPad", "iPod"]
        .iter()
        .any(|device| user_agent.contains(device))
}

/// The number at the start of what follows the first `prefix` in `s`.
fn version_after(s: &str, prefix: &str) -> Option<u16> {
    let rest = &s[s.find(prefix)? + prefix.len()..];
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    rest[..digits].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn browser(engine: BrowserEngine, major_version: u16) -> Browser {
        Browser {
            engine,
            major_version: Some(major_version),
        }
    }

    #[test]
    fn desktop_browsers() {
        use BrowserEngine::*;
        let cases = [
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
                browser(Blink, 141),
                Os::Windows,
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 Edg/141.0.0.0",
                browser(Blink, 141),
                Os::Windows,
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0",
                browser(Gecko, 140),
                Os::Linux,
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10.15; rv:143.0) Gecko/20100101 Firefox/143.0",
                browser(Gecko, 143),
                Os::MacOs,
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15",
                browser(WebKit, 26),
                Os::MacOs,
            ),
            (
                "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
                browser(Blink, 140),
                Os::Linux,
            ),
            (
                "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36 OPR/125.0.0.0",
                browser(Blink, 141),
                Os::Linux,
            ),
        ];
        for (user_agent, expected_browser, expected_os) in cases {
            assert_eq!(
                detect(user_agent, 0),
                (expected_browser, expected_os),
                "{user_agent}"
            );
        }
    }

    #[test]
    fn mobile_browsers() {
        use BrowserEngine::*;
        let cases = [
            (
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Mobile Safari/537.36",
                browser(Blink, 141),
                Os::Android,
            ),
            (
                "Mozilla/5.0 (Android 15; Mobile; rv:143.0) Gecko/143.0 Firefox/143.0",
                browser(Gecko, 143),
                Os::Android,
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Mobile/15E148 Safari/604.1",
                browser(WebKit, 18),
                Os::Ios,
            ),
            // Chrome on iOS uses WebKit and says no Safari version.
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/141.0.7390.41 Mobile/15E148 Safari/604.1",
                browser(WebKit, 18),
                Os::Ios,
            ),
            (
                "Mozilla/5.0 (iPad; CPU OS 17_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) FxiOS/143.0 Mobile/15E148 Safari/605.1.15",
                browser(WebKit, 17),
                Os::Ios,
            ),
        ];
        for (user_agent, expected_browser, expected_os) in cases {
            assert_eq!(
                detect(user_agent, 5),
                (expected_browser, expected_os),
                "{user_agent}"
            );
        }
    }

    #[test]
    fn ipad_with_a_desktop_user_agent() {
        let user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Safari/605.1.15";
        assert_eq!(detect(user_agent, 5).1, Os::Ios);
        assert_eq!(detect(user_agent, 0).1, Os::MacOs);
        assert_eq!(detect(user_agent, 5).0, browser(BrowserEngine::WebKit, 18));
    }

    #[test]
    fn unknown_user_agents() {
        let unknown = Browser {
            engine: BrowserEngine::Unknown,
            major_version: None,
        };
        assert_eq!(detect("", 0), (unknown, Os::Unknown));
        assert_eq!(detect("curl/8.5.0", 0), (unknown, Os::Unknown));
        // A version too large for `u16` is dropped, not wrapped.
        assert_eq!(
            detect("Mozilla/5.0 (X11; Linux x86_64) Firefox/99999999", 0).0,
            Browser {
                engine: BrowserEngine::Gecko,
                major_version: None,
            }
        );
    }
}
