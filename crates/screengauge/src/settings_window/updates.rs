//! "Check for updates" in Settings' footer (WPC-335): what its click does, and what the line under the footer says.
//! In the installer copy the click asks GitHub on a thread of its own, so Settings keeps answering while it waits; the
//! line says "Checking…" until the thread tells the window GitHub's answer. In the Store copy the click opens the app's
//! page in the Microsoft Store, and nothing goes to the internet. Nothing here happens without the click.
//!
//! The line goes with the window: closed and opened again, Settings shows no line until the next click, and an answer
//! still on its way then reaches nobody.

use std::cell::{Cell, RefCell};
use std::panic;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use windows::Win32::Foundation::HWND;

use super::footer::Status;
use crate::update_check::{self, Answer, Click};
use crate::{app_message, log, package};

thread_local! {
    /// What the line under the footer says, from the click on, while the window is open.
    static STATUS: Cell<Option<Status>> = const { Cell::new(None) };
    /// Where the check's thread puts GitHub's answer, while it is asking. Dropped with the window, so a late answer
    /// reaches nobody.
    static ANSWER: RefCell<Option<Receiver<Answer>>> = const { RefCell::new(None) };
}

/// What the line under the footer says now, if anything.
pub fn status() -> Option<Status> {
    STATUS.get()
}

/// A click on "Check for updates" in Settings, `hwnd`.
pub fn click(hwnd: HWND) {
    match update_check::on_click(package::this_copy()) {
        Click::OpenStore => super::open_address(hwnd, update_check::STORE_PAGE, "the Store's page"),
        Click::AskGitHub => ask(hwnd),
        Click::CouldntCheck => show(hwnd, Status::Answered(Answer::CouldntCheck)),
    }
}

/// Asks GitHub on a thread of its own, and says "Checking…" meanwhile. A click while it asks does nothing more, unless
/// the answer already came and its message was lost, or the thread ended with none: then the click shows the answer,
/// or asks again, so "Checking…" never stays for good.
fn ask(hwnd: HWND) {
    if STATUS.get() == Some(Status::Checking) {
        match received() {
            Ok(answer) => {
                show(hwnd, Status::Answered(answer));
                return;
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                log::failure(
                    "Check for updates",
                    &"its thread ended without an answer; asking again",
                );
            }
        }
    }
    let (sender, answer) = mpsc::channel();
    let window = hwnd.0.expose_provenance();
    let spawned = thread::Builder::new()
        .name("check for updates".to_owned())
        .spawn(move || {
            // A panic in the check still answers, so the line doesn't say "Checking…" for good.
            let answer = panic::catch_unwind(update_check::check).unwrap_or_else(|_| {
                log::failure("Check for updates", &"the check panicked");
                Answer::CouldntCheck
            });
            // Settings closed while GitHub was asked: nobody to tell.
            if sender.send(answer).is_err() {
                return;
            }
            let window = HWND(std::ptr::with_exposed_provenance_mut(window));
            // The window may be gone already, which the post reports.
            if let Err(e) = app_message::post(window, app_message::UPDATE_CHECKED) {
                log::failure("Check for updates: telling Settings the answer", &e);
            }
        });
    match spawned {
        Ok(_) => {
            ANSWER.set(Some(answer));
            show(hwnd, Status::Checking);
        }
        Err(e) => {
            log::failure("Check for updates: starting its thread", &e);
            show(hwnd, Status::Answered(Answer::CouldntCheck));
        }
    }
}

/// [`CHECKED`] came: the line says GitHub's answer. A message from a check Settings no longer waits for, closed and
/// opened again since, finds no answer and changes nothing.
pub fn on_checked(hwnd: HWND) {
    if let Ok(answer) = received() {
        show(hwnd, Status::Answered(answer));
    }
}

/// The answer of the check Settings waits for, once, should it have come.
fn received() -> Result<Answer, TryRecvError> {
    let answer = ANSWER.with_borrow(|answer| {
        answer
            .as_ref()
            .map_or(Err(TryRecvError::Disconnected), Receiver::try_recv)
    });
    if answer.is_ok() {
        ANSWER.set(None);
    }
    answer
}

/// The line says `status`: the window grows to hold it the first time, and is drawn again after.
fn show(hwnd: HWND, status: Status) {
    if STATUS.replace(Some(status)).is_none() {
        super::follow_page(hwnd);
    } else {
        super::invalidate(hwnd);
    }
}

/// Settings closes: the line goes, and an answer on its way reaches nobody.
pub fn close() {
    STATUS.set(None);
    ANSWER.set(None);
}
