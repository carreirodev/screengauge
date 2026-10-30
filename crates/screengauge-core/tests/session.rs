//! Layer 1 tests of the session core (v1 spec WPC-296, Testing Decisions): opening and closing the tools, with frozen
//! screens and monitor arrangements made up here, without a screen. They look only at what would be drawn and at the
//! effects the Windows layer would carry out.

use screengauge_core::session::{
    Colors, Cursor, Effect, Event, Held, Image, Input, Key, LocalTime, Opening, Photo, Point, Rect,
    Refused, Scale, Session,
};
use screengauge_core::settings::{Options, Remembered, Tool};

/// The owner's 4K monitor at 175%, Windows' main monitor.
const AOC: Rect = Rect {
    left: 0,
    top: 0,
    width: 3840,
    height: 2160,
};
/// The owner's Full HD monitor at 125%, to the left of the 4K.
const ARZOPA: Rect = Rect {
    left: -1920,
    top: 0,
    width: 1920,
    height: 1080,
};

/// A photo of `rect` painted in one shade of blue, so each monitor's photo is told apart.
fn photo(rect: Rect, dpi: u32, blue: u8) -> Photo {
    let pixels = usize::try_from(rect.width * rect.height).expect("a small monitor");
    let image = Image::new(rect.width, rect.height, [blue, 0, 0, 255].repeat(pixels))
        .expect("the pixels fill the image");
    Photo {
        rect,
        scale: Scale::from_dpi(dpi).expect("a real scale"),
        colors: Colors::Exact,
        image,
    }
}

/// The owner's two monitors: the 4K at 175% (168 DPI) in blue 1, the Full HD at 125% (120 DPI) in blue 2.
fn owners_monitors() -> Vec<Photo> {
    vec![photo(AOC, 168, 1), photo(ARZOPA, 120, 2)]
}

/// Opening with `frozen_screen`, every monitor's photo taken, the default options and nothing remembered yet.
fn opening(frozen_screen: Vec<Photo>) -> Opening {
    Opening {
        frozen_screen: frozen_screen.into_iter().map(Some).collect(),
        options: Options::default(),
        remembered: Remembered::default(),
    }
}

fn open(frozen_screen: Vec<Photo>) -> Session {
    Session::open(opening(frozen_screen)).expect("a frozen screen the session takes")
}

#[test]
fn opening_shows_every_monitor_its_own_photo_in_its_place() {
    let session = open(owners_monitors());
    let shown: Vec<_> = session
        .scene()
        .frozen_screen
        .iter()
        .map(|photo| {
            (
                photo.rect,
                photo.scale.dpi(),
                photo.image.width(),
                photo.image.height(),
                photo.image.bgra()[0],
            )
        })
        .collect();
    assert_eq!(
        shown,
        [(AOC, 168, 3840, 2160, 1), (ARZOPA, 120, 1920, 1080, 2)]
    );
}

#[test]
fn opening_refuses_a_photo_that_isnt_its_monitors_size() {
    let mut frozen_screen = owners_monitors();
    // The Full HD monitor's photo, one row short.
    frozen_screen[1].image =
        Image::new(1920, 1079, vec![0; 1920 * 1079 * 4]).expect("a whole image");
    assert_eq!(
        Session::open(opening(frozen_screen)).err(),
        Some(Refused::PhotoSize(1))
    );
}

#[test]
fn opening_refuses_a_frozen_screen_without_monitors() {
    assert_eq!(
        Session::open(opening(Vec::new())).err(),
        Some(Refused::NoMonitor)
    );
}

#[test]
fn a_monitor_left_without_a_photo_keeps_the_tools_closed() {
    // All or nothing (v1 spec WPC-296, Frozen screen): one monitor that neither capture path could photograph is
    // enough, whichever it is, even when the other monitor's photo came through.
    let cases = [
        (vec![Some(photo(AOC, 168, 1)), None], 1),
        (vec![None, Some(photo(ARZOPA, 120, 2))], 0),
        (vec![None, None], 0),
    ];
    for (frozen_screen, monitor) in cases {
        let opening = Opening {
            frozen_screen,
            ..opening(Vec::new())
        };
        assert_eq!(
            Session::open(opening).err(),
            Some(Refused::NotCaptured(monitor))
        );
    }
}

#[test]
fn an_image_holds_exactly_its_pixels() {
    assert!(Image::new(2, 2, vec![0; 16]).is_some());
    assert!(Image::new(2, 2, vec![0; 15]).is_none());
    assert!(Image::new(2, 2, vec![0; 20]).is_none());
}

#[test]
fn the_cursor_over_the_frozen_screen_is_the_thin_cross() {
    assert_eq!(open(owners_monitors()).scene().cursor, Cursor::Cross);
}

/// `input` with the cursor in the middle of the 4K monitor, no modifier held, at 20:41:12 on 2026-10-26.
fn event(input: Input) -> Event {
    Event {
        cursor: Point { x: 1920, y: 1080 },
        held: Held::default(),
        time: LocalTime {
            year: 2026,
            month: 10,
            day: 26,
            hour: 20,
            minute: 41,
            second: 12,
        },
        input,
    }
}

#[test]
fn esc_closes_the_tools_whichever_tool_was_last_used() {
    for last_tool in [Tool::Dimensions, Tool::Bounds, Tool::Inspect, Tool::Snip] {
        let mut session = Session::open(Opening {
            remembered: Remembered {
                last_tool,
                ..Remembered::default()
            },
            ..opening(owners_monitors())
        })
        .expect("a frozen screen the session takes");
        // Closing, and nothing else: nothing copied or saved, and nothing remembered changed, so nothing to write.
        assert_eq!(
            session.handle(event(Input::KeyDown(Key::Esc))),
            [Effect::Close],
            "{last_tool:?}"
        );
    }
}

#[test]
fn the_global_shortcut_again_closes_the_tools() {
    let mut session = open(owners_monitors());
    assert_eq!(
        session.handle(event(Input::GlobalShortcut)),
        [Effect::Close]
    );
}

#[test]
fn calling_again_by_the_tray_icon_or_by_opening_screengauge_changes_nothing() {
    let mut session = open(owners_monitors());
    // No effect: the tools stay open on the same photo, which isn't taken again.
    assert_eq!(session.handle(event(Input::Called)), []);
    assert_eq!(
        session.scene().frozen_screen[0].image.bgra()[0],
        1,
        "the 4K monitor's photo"
    );
    assert_eq!(
        session.handle(event(Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
}

#[test]
fn alt_f4_closes_the_tools_as_a_whole() {
    let mut session = open(owners_monitors());
    assert_eq!(session.handle(event(Input::CloseAsked)), [Effect::Close]);
}

#[test]
fn nothing_happens_once_the_tools_closed() {
    let mut session = open(owners_monitors());
    assert_eq!(
        session.handle(event(Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
    // Esc repeating while held, then the global shortcut and Alt+F4 before the Windows layer closed the windows.
    for input in [
        Input::KeyDown(Key::Esc),
        Input::GlobalShortcut,
        Input::CloseAsked,
    ] {
        assert_eq!(session.handle(event(input.clone())), [], "{input:?}");
    }
}

#[test]
fn moving_clicking_turning_the_wheel_and_other_keys_leave_the_tools_open() {
    let mut session = open(owners_monitors());
    let keys = [
        Key::H,
        Key::V,
        Key::C,
        Key::Digit1,
        Key::Digit2,
        Key::Digit3,
        Key::Digit4,
        Key::Left,
        Key::Right,
        Key::Up,
        Key::Down,
        Key::Backspace,
        Key::Delete,
        Key::Shift,
        Key::Alt,
    ];
    let inputs = [
        Input::Move,
        Input::Press,
        Input::Release,
        Input::Wheel(120),
        Input::Wheel(-120),
        // Esc coming up after the tools ignored it going down elsewhere.
        Input::KeyUp(Key::Esc),
    ]
    .into_iter()
    .chain(keys.iter().map(|&key| Input::KeyDown(key)))
    .chain(keys.iter().map(|&key| Input::KeyUp(key)));
    for input in inputs {
        assert_eq!(session.handle(event(input.clone())), [], "{input:?}");
        assert_eq!(session.scene().cursor, Cursor::Cross, "{input:?}");
    }
    assert_eq!(
        session.handle(event(Input::KeyDown(Key::Esc))),
        [Effect::Close]
    );
}
