//! "Check for updates" in Settings' footer (v1 spec WPC-296, Settings; WPC-335): in the installer copy, a click asks
//! GitHub for the project's latest release and says whether a newer version is out. It is ScreenGauge's one trip to the
//! internet, and only a click makes it.

use std::ffi::c_void;
use std::fmt;

use windows::Data::Json::JsonObject;
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::Networking::WinHttp::{
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
};
use windows::core::{Error, HSTRING, PCWSTR, w};

use crate::package::AppCopy;
use crate::{com, log};

/// A release's version, from its three numbers: what ScreenGauge compares and shows, never the text GitHub sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// `1.2.3`, or a release's tag `v1.2.3`; `None` for anything else.
    pub fn parse(text: &str) -> Option<Version> {
        let digits = text.strip_prefix('v').unwrap_or(text);
        let mut numbers = digits.split('.').map(|number| {
            // `u32::from_str` takes a leading `+`, which no version has.
            if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            number.parse().ok()
        });
        let version = Version {
            major: numbers.next()??,
            minor: numbers.next()??,
            patch: numbers.next()??,
        };
        match numbers.next() {
            None => Some(version),
            Some(_) => None,
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What the footer's line says once GitHub answered, or didn't.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// "You're on the latest version."
    Latest,
    /// "Version X is available. Download"
    Available(Version),
    /// "Couldn't check. Check your connection and try again."
    CouldntCheck,
}

/// What a click on "Check for updates" does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Click {
    /// The Store copy: the app's page in the Microsoft Store, which keeps it up to date. ScreenGauge itself doesn't go
    /// to the internet, so it never announces a version the Store hasn't approved yet.
    OpenStore,
    /// The installer copy: ask GitHub for the latest release.
    AskGitHub,
    /// Windows couldn't say which copy this is: "Couldn't check…", asking nobody.
    CouldntCheck,
}

/// What a click does in the copy Windows says this is (`package::this_copy`).
pub fn on_click(copy: windows::core::Result<AppCopy>) -> Click {
    match copy {
        Ok(AppCopy::Store) => Click::OpenStore,
        Ok(AppCopy::Installer) => Click::AskGitHub,
        Err(e) => {
            log::failure("Check for updates: which copy this is", &e);
            Click::CouldntCheck
        }
    }
}

/// Asks GitHub for the latest release, and says what the footer's line shows. It waits for GitHub, up to `TIMEOUT` for
/// each step, so it runs on a thread of its own. Why it couldn't check goes to the log.
pub fn check() -> Answer {
    let Some(running) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        log::failure(
            "Check for updates",
            &"this exe's own version isn't a version",
        );
        return Answer::CouldntCheck;
    };
    let answer = fetch()
        .map_err(|e| e.to_string())
        .and_then(|reply| read(&reply, running));
    match answer {
        Ok(answer) => answer,
        Err(why) => {
            log::failure("Check for updates", &why);
            Answer::CouldntCheck
        }
    }
}

/// GitHub's API, and the address of the repository's latest release there: the repository the workspace's manifest
/// names (a test holds them together).
const HOST: &str = "api.github.com";
const LATEST_RELEASE: &str = "/repos/carreirodev/screengauge/releases/latest";

/// What "Download" opens in the browser: the latest release's page, with the installer.
pub const RELEASES_PAGE: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest");

/// What the Store copy's click opens: the app's page in the Microsoft Store, by its Store ID, the way Microsoft
/// recommends (`ms-windows-store://pdp/?ProductId=`).
pub const STORE_PAGE: &str = "ms-windows-store://pdp/?ProductId=9PGNSRZ2BPJ2";

/// GitHub refuses a request with no User-Agent (HTTP 403); this one names only the program and its version.
const USER_AGENT: &str = concat!("ScreenGauge/", env!("CARGO_PKG_VERSION"));

/// The JSON GitHub's API documents, at the API's version that documents it.
const HEADERS: &str = "Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28";

/// How long each step may take, in milliseconds: finding GitHub's address, connecting, sending and receiving. Windows
/// waits forever for the address otherwise, and "Checking…" would never go.
const TIMEOUT: i32 = 10_000;

/// The most of a reply ScreenGauge reads. A release's JSON is a few kilobytes, its notes and files' list included.
const MOST: usize = 1 << 20;

/// GitHub's reply to the latest release's address: its HTTP status and its body.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reply {
    status: u32,
    body: String,
}

/// A WinHTTP handle, closed when it drops.
struct Handle(*mut c_void);

impl Handle {
    /// The handle a WinHTTP call returned, or the error it left when it returned none.
    fn new(raw: *mut c_void) -> windows::core::Result<Handle> {
        if raw.is_null() {
            Err(Error::from_thread())
        } else {
            Ok(Handle(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: a handle WinHTTP returned, closed once.
        if let Err(e) = unsafe { WinHttpCloseHandle(self.0) } {
            log::failure("Check for updates: closing a WinHTTP handle", &e);
        }
    }
}

/// GETs the latest release from GitHub's API over HTTPS, through the proxy Windows is set to use, with Windows' own
/// certificate checks.
fn fetch() -> windows::core::Result<Reply> {
    let too_large = || Error::new(E_FAIL, "the reply is larger than a release");
    let past_u32 = || Error::new(E_FAIL, "a size past what WinHTTP counts");
    // SAFETY: static strings that outlive the call. No proxy named: Windows' own setting decides.
    let session = Handle::new(unsafe {
        WinHttpOpen(
            &HSTRING::from(USER_AGENT),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    })?;
    // SAFETY: a live session handle.
    unsafe { WinHttpSetTimeouts(session.0, TIMEOUT, TIMEOUT, TIMEOUT, TIMEOUT) }?;
    // SAFETY: a live session handle and a string that outlives the call.
    let connection = Handle::new(unsafe {
        WinHttpConnect(
            session.0,
            &HSTRING::from(HOST),
            INTERNET_DEFAULT_HTTPS_PORT,
            0,
        )
    })?;
    // SAFETY: a live connection handle and strings that outlive the call. No version, referrer or accepted types named:
    // HTTP/1.1, none, and the Accept header below.
    let request = Handle::new(unsafe {
        WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &HSTRING::from(LATEST_RELEASE),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    })?;
    let headers: Vec<u16> = HEADERS.encode_utf16().collect();
    // SAFETY: a live request handle; the headers outlive the call, which reads exactly their length. No body.
    unsafe { WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0) }?;
    // SAFETY: a live request handle, its request sent.
    unsafe { WinHttpReceiveResponse(request.0, std::ptr::null_mut()) }?;
    let mut status = 0u32;
    let mut size = u32::try_from(size_of::<u32>()).map_err(|_| past_u32())?;
    // SAFETY: a live request handle with its response; the status is read as a number into a local of `size` bytes.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&raw mut status).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
    }?;
    let mut body = Vec::new();
    loop {
        let mut available = 0u32;
        // SAFETY: a live request handle with its response; fills a local.
        unsafe { WinHttpQueryDataAvailable(request.0, &mut available) }?;
        if available == 0 {
            break;
        }
        let start = body.len();
        let end = usize::try_from(available)
            .ok()
            .and_then(|available| start.checked_add(available))
            .filter(|&end| end <= MOST)
            .ok_or_else(too_large)?;
        body.resize(end, 0);
        let mut read = 0u32;
        // SAFETY: the buffer from `start` holds the `available` bytes asked for, and outlives the call.
        unsafe {
            WinHttpReadData(
                request.0,
                body[start..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
        }?;
        body.truncate(start + usize::try_from(read).map_err(|_| past_u32())?);
    }
    let body =
        String::from_utf8(body).map_err(|_| Error::new(E_FAIL, "the reply isn't UTF-8 text"))?;
    Ok(Reply { status, body })
}

/// What `reply` says for a ScreenGauge at `running`, or why it says nothing ScreenGauge can trust.
fn read(reply: &Reply, running: Version) -> Result<Answer, String> {
    match reply.status {
        200 => {}
        // GitHub has no release yet, or only drafts and prereleases, which the latest release leaves out: nothing is
        // newer than what the user has (decided in WPC-335).
        404 => return Ok(Answer::Latest),
        status => return Err(format!("GitHub answered HTTP {status}")),
    }
    com::keep_multithreaded_apartment()
        .map_err(|e| format!("COM's multithreaded apartment can't be kept open: {e}"))?;
    let release = JsonObject::Parse(&HSTRING::from(&reply.body))
        .map_err(|e| format!("the reply isn't a JSON object: {e}"))?;
    let tag = release
        .GetNamedString(&HSTRING::from("tag_name"))
        .map_err(|e| format!("the reply has no tag_name: {e}"))?;
    let latest = Version::parse(&tag.to_string_lossy())
        .ok_or_else(|| "the latest release's tag isn't a version".to_string())?;
    Ok(if latest > running {
        Answer::Available(latest)
    } else {
        Answer::Latest
    })
}

#[cfg(test)]
mod tests {
    use std::thread;

    use windows::Win32::Foundation::E_FAIL;
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

    use super::*;
    use crate::test_process;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a version")
    }

    /// GitHub's reply with a release tagged `tag`, cut to the fields around the one ScreenGauge reads.
    fn release(tag: &str) -> Reply {
        Reply {
            status: 200,
            body: format!(
                concat!(
                    r#"{{"url":"https://api.github.com/repos/carreirodev/screengauge/releases/1","#,
                    r#""tag_name":"{tag}","name":"ScreenGauge {tag}","draft":false,"prerelease":false,"assets":[]}}"#
                ),
                tag = tag
            ),
        }
    }

    #[test]
    fn a_newer_release_on_github_is_available() {
        assert_eq!(
            read(&release("v1.1.0"), version("1.0.0")),
            Ok(Answer::Available(version("1.1.0")))
        );
    }

    #[test]
    fn the_same_release_or_an_older_one_is_the_latest_version() {
        for tag in ["v1.0.0", "1.0.0", "v0.9.9"] {
            assert_eq!(
                read(&release(tag), version("1.0.0")),
                Ok(Answer::Latest),
                "{tag}"
            );
        }
    }

    #[test]
    fn versions_compare_as_numbers_not_as_text() {
        // As text, "1.9.0" comes after "1.10.0".
        assert_eq!(
            read(&release("v1.10.0"), version("1.9.0")),
            Ok(Answer::Available(version("1.10.0")))
        );
        assert_eq!(
            read(&release("v1.9.0"), version("1.10.0")),
            Ok(Answer::Latest)
        );
    }

    #[test]
    fn the_version_shown_is_rebuilt_from_its_numbers() {
        let Ok(Answer::Available(newer)) = read(&release("v2.0.1"), version("0.1.0")) else {
            panic!("a newer release");
        };
        assert_eq!(newer.to_string(), "2.0.1");
    }

    #[test]
    fn with_no_release_published_github_says_not_found_and_that_is_the_latest_version() {
        // GitHub's whole reply for carreirodev/screengauge on 2026-11-03, before the first release.
        let none = Reply {
            status: 404,
            body: concat!(
                r#"{"message":"Not Found","#,
                r#""documentation_url":"https://docs.github.com/rest/releases/releases#get-the-latest-release","#,
                r#""status":"404"}"#
            )
            .to_string(),
        };
        assert_eq!(read(&none, version("0.1.0")), Ok(Answer::Latest));
    }

    #[test]
    fn any_other_status_can_t_be_trusted_even_with_a_release_in_it() {
        // 403 is GitHub's limit of 60 checks an hour from one address, 429 the same, and 5xx GitHub down. A proxy's
        // page may be anything.
        for status in [301, 403, 429, 500, 502, 503] {
            let mut reply = release("v9.0.0");
            reply.status = status;
            assert!(read(&reply, version("1.0.0")).is_err(), "{status}");
        }
    }

    #[test]
    fn a_reply_that_isn_t_a_release_can_t_be_trusted() {
        for body in [
            "",
            "<html>Sign in to the network</html>",
            "[]",
            r#"{"name":"no tag"}"#,
            r#"{"tag_name":12}"#,
            r#"{"tag_name":"latest"}"#,
            r#"{"tag_name":"v1.2"}"#,
            r#"{"tag_name":"v1.2.3.4"}"#,
            r#"{"tag_name":"v1.2.3-beta"}"#,
            r#"{"tag_name":"v1.+2.3"}"#,
            r#"{"tag_name":"v1.2.99999999999"}"#,
        ] {
            let reply = Reply {
                status: 200,
                body: body.to_string(),
            };
            assert!(read(&reply, version("1.0.0")).is_err(), "{body}");
        }
    }

    #[test]
    fn in_the_store_copy_a_click_opens_the_store_and_never_asks_github() {
        assert_eq!(on_click(Ok(AppCopy::Store)), Click::OpenStore);
    }

    #[test]
    fn in_the_installer_copy_a_click_asks_github() {
        assert_eq!(on_click(Ok(AppCopy::Installer)), Click::AskGitHub);
    }

    #[test]
    fn when_windows_can_t_say_which_copy_this_is_a_click_asks_nobody() {
        // Taking it for the installer copy could have the Store copy announce a version the Store hasn't approved.
        assert_eq!(
            on_click(Err(windows::core::Error::from(E_FAIL))),
            Click::CouldntCheck
        );
    }

    #[test]
    fn github_is_asked_about_the_repository_the_manifest_names_and_download_opens_its_releases() {
        let repository = env!("CARGO_PKG_REPOSITORY")
            .strip_prefix("https://github.com/")
            .expect("the repository is on GitHub");
        assert_eq!(repository, "carreirodev/screengauge");
        assert_eq!(
            LATEST_RELEASE,
            format!("/repos/{repository}/releases/latest")
        );
        assert_eq!(
            RELEASES_PAGE,
            "https://github.com/carreirodev/screengauge/releases/latest"
        );
    }

    /// The real question to GitHub, as a click asks it, on a thread of its own as in Settings.
    #[test]
    #[ignore = "goes to the internet"]
    fn github_answers_the_real_question() {
        let answer = std::thread::spawn(check)
            .join()
            .expect("the check's thread");
        // Before the first release GitHub says it has none, which is the latest version (decided in WPC-335).
        assert_ne!(answer, Answer::CouldntCheck);
        println!("GitHub's answer: {answer:?}");
    }

    #[test]
    fn this_exe_s_own_version_is_a_version() {
        assert!(Version::parse(env!("CARGO_PKG_VERSION")).is_some());
    }

    #[test]
    fn a_reply_still_reads_after_another_thread_read_one_and_ended_its_com() {
        test_process::run_alone(concat!(
            module_path!(),
            "::alone_a_reply_still_reads_after_another_thread_read_one_and_ended_its_com"
        ));
    }

    /// One thread starts COM's multithreaded apartment by hand, reads a reply in it and ends COM; then a thread with no
    /// COM reads one. Alone, because another test holding COM open would hide what happens once nothing does.
    #[test]
    #[ignore = "the shared run starts it alone, in a process of its own, from the test above"]
    fn alone_a_reply_still_reads_after_another_thread_read_one_and_ended_its_com() {
        let first = thread::spawn(|| {
            // SAFETY: no reserved pointer; undone below, on this thread.
            unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
                .ok()
                .expect("COM starts on the first thread");
            let answer = read(&release("v1.1.0"), version("1.0.0"));
            // SAFETY: pairs the CoInitializeEx above; the reply's objects are gone with `read`.
            unsafe { CoUninitialize() };
            answer
        })
        .join()
        .expect("the first thread");
        assert_eq!(first, Ok(Answer::Available(version("1.1.0"))));
        let second = thread::spawn(|| read(&release("v1.2.0"), version("1.0.0")))
            .join()
            .expect("the thread with no COM");
        assert_eq!(second, Ok(Answer::Available(version("1.2.0"))));
    }
}
