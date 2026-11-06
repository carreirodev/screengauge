//! What the user does over a frozen screen window, handed to the session core: the cursor's shape, the mouse, with a
//! press on the toolbar kept from the tool, the wheel and the keys.

use screengauge_core::session::toolbar::Hit;
use screengauge_core::session::{Input, Key};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    HTCLIENT, WM_CAPTURECHANGED, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_SETCURSOR, WM_SYSKEYDOWN, WM_SYSKEYUP,
};

use super::{ALT_ESC, Window, redraw_window};
use crate::win32::low_word;
use crate::{event, log};

/// What the left button went down on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Pressed {
    /// Something on the toolbar. A button acts only when the mouse comes up on the same button (WPC-310), and looks
    /// pressed meanwhile while the cursor is on it (WPC-311). Held on the background, the toolbar is being dragged, and
    /// the mouse coming up lets it go (WPC-312).
    Toolbar(Hit),
    /// The tool, off the toolbar: a click in Dimensions, or the corner a rectangle in Bounds is drawn from, whose drag
    /// ends where the mouse comes up, on this monitor or another (WPC-313).
    Tool,
}

impl Pressed {
    /// What on the toolbar is held, which looks pressed while the cursor is on it.
    pub(super) fn on_toolbar(self) -> Option<Hit> {
        match self {
            Pressed::Toolbar(hit) => Some(hit),
            Pressed::Tool => None,
        }
    }
}

/// Hands `message` to the session core when it is the mouse, the wheel or a key over `window`, and says what the window
/// procedure returns for it: `None` for Windows' default, and for any other message, which the procedure handles.
pub(super) fn route(
    hwnd: HWND,
    window: &Window,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> Option<LRESULT> {
    let deliver = |input: Input| {
        window.session.input(input);
        Some(LRESULT(0))
    };
    match message {
        WM_SETCURSOR if low_word(lparam) == HTCLIENT => {
            window.session.show_cursor();
            Some(LRESULT(1))
        }
        WM_MOUSEMOVE => deliver(Input::Move),
        // A press anywhere on the toolbar is the toolbar's: it never reaches the tool, which would pin and copy what
        // lies under the toolbar. Wherever the press is, the window keeps the mouse until the button comes up, so a
        // drag of the toolbar, or of a rectangle in Bounds, goes on over the other monitors' windows and its release
        // comes here.
        WM_LBUTTONDOWN => {
            let hit = window.session.toolbar_hit(event::message_cursor());
            window
                .pressed
                .set(Some(hit.map_or(Pressed::Tool, Pressed::Toolbar)));
            // SAFETY: our own window, on its thread, with the left button down. It returns the window that had the
            // mouse before, which needs nothing.
            let _ = unsafe { SetCapture(hwnd) };
            match hit {
                Some(hit) => {
                    // The button under the cursor looks pressed.
                    redraw_window(hwnd);
                    match hit {
                        // On the background, the press picks the toolbar up, and the core drags it with the cursor.
                        Hit::Background => deliver(Input::Press),
                        Hit::Button(_) => Some(LRESULT(0)),
                    }
                }
                None => deliver(Input::Press),
            }
        }
        // A toolbar button acts when the mouse comes up on it, as a Windows button does, so moving off it first
        // cancels. Neither the press nor its release reaches the tool.
        // `pressed` is taken before the mouse is given back: ReleaseCapture sends WM_CAPTURECHANGED to this window at
        // once, which clears it.
        WM_LBUTTONUP => {
            let pressed = window.pressed.take();
            if pressed.is_some() {
                // SAFETY: gives back the mouse this window took at the press, on its thread.
                if let Err(e) = unsafe { ReleaseCapture() } {
                    log::failure("frozen screen: giving back the mouse", &e);
                }
            }
            match pressed {
                Some(Pressed::Toolbar(held)) => {
                    let released = window.session.toolbar_hit(event::message_cursor());
                    // The button no longer looks pressed, whatever the click does.
                    redraw_window(hwnd);
                    match held {
                        Hit::Button(button) if released == Some(held) => {
                            deliver(Input::Toolbar(button))
                        }
                        Hit::Button(_) => Some(LRESULT(0)),
                        // The dragged toolbar is let go where it is, whatever the cursor is on now.
                        Hit::Background => deliver(Input::Release),
                    }
                }
                // A rectangle in Bounds ends where the mouse comes up, on the toolbar too.
                Some(Pressed::Tool) | None => deliver(Input::Release),
            }
        }
        // Another window took the mouse while the button was held, so its release may never come here: a toolbar
        // button held down goes back to rest, and a dragged toolbar is let go where it is, or it would follow the
        // cursor with the button up. A rectangle in Bounds is pinned and copied only when the button really comes
        // up, over one of these windows: losing the mouse alone, as to the Windows key, which then closes the tools,
        // copies nothing, and the clipboard stays as it was (the owner's choice in WPC-313).
        WM_CAPTURECHANGED => match window.pressed.take() {
            Some(Pressed::Toolbar(Hit::Background)) => {
                redraw_window(hwnd);
                deliver(Input::Release)
            }
            Some(Pressed::Toolbar(Hit::Button(_))) => {
                redraw_window(hwnd);
                Some(LRESULT(0))
            }
            Some(Pressed::Tool) | None => Some(LRESULT(0)),
        },
        WM_MOUSEWHEEL => deliver(Input::Wheel(event::wheel(wparam))),
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP => {
            let key = event::key(wparam)?;
            window
                .session
                .input(if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                    Input::KeyDown(key)
                } else {
                    Input::KeyUp(key)
                });
            // Alt, and keys pressed with it, come as system keys. Windows handles them afterwards as before, so
            // Alt+F4 still turns into WM_CLOSE.
            matches!(message, WM_KEYDOWN | WM_KEYUP).then_some(LRESULT(0))
        }
        // Alt+Esc, which the first window holds while the tools are open: the tools' Esc.
        WM_HOTKEY if i32::try_from(wparam.0) == Ok(ALT_ESC) => deliver(Input::KeyDown(Key::Esc)),
        _ => None,
    }
}
