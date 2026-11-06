//! Layer 1 tests of Snip saving its image to a file (v1 spec WPC-296, Snip; ticket WPC-318): the automatic name from the
//! time the test gives, the numbers a taken name gets, the `.png` a name chosen in Save As always ends with, what each
//! Destination and "Save to default folder" do as the button comes up, Save As, and the notices, on frozen screens drawn
//! here, without a screen or a disk. They look only at the scene the session would draw and at the effects it returns.

use std::path::{Path, PathBuf};

use screengauge_core::session::{FileNames, LocalTime, png_path};

/// 20:41:12 on 2026-10-26, the ticket's example.
const EVENING: LocalTime = LocalTime {
    year: 2026,
    month: 10,
    day: 26,
    hour: 20,
    minute: 41,
    second: 12,
};

#[test]
fn the_automatic_name_is_the_local_date_and_time_to_the_second() {
    assert_eq!(
        FileNames::at(EVENING).first(),
        "screengauge-20261026-204112.png"
    );
}

#[test]
fn every_part_of_the_time_keeps_its_leading_zeros() {
    let early = LocalTime {
        year: 2027,
        month: 1,
        day: 2,
        hour: 3,
        minute: 4,
        second: 5,
    };
    assert_eq!(
        FileNames::at(early).first(),
        "screengauge-20270102-030405.png"
    );
}

#[test]
fn a_taken_name_is_followed_by_2_then_3_and_so_on() {
    let names: Vec<String> = FileNames::at(EVENING).iter().take(4).collect();
    assert_eq!(
        names,
        [
            "screengauge-20261026-204112.png",
            "screengauge-20261026-204112-2.png",
            "screengauge-20261026-204112-3.png",
            "screengauge-20261026-204112-4.png",
        ]
    );
}

#[test]
fn a_name_chosen_without_an_extension_gets_png() {
    assert_eq!(
        png_path(Path::new(r"C:\Users\Ana\Pictures\button")),
        PathBuf::from(r"C:\Users\Ana\Pictures\button.png")
    );
}

#[test]
fn another_extension_is_replaced_by_png() {
    assert_eq!(
        png_path(Path::new(r"C:\Users\Ana\Pictures\button.jpg")),
        PathBuf::from(r"C:\Users\Ana\Pictures\button.png")
    );
}

#[test]
fn a_png_name_stays_as_it_was_whatever_its_case() {
    for name in [r"D:\Snips\button.png", r"D:\Snips\button.PNG"] {
        assert_eq!(png_path(Path::new(name)), PathBuf::from(name));
    }
}

#[test]
fn only_the_last_extension_is_replaced() {
    assert_eq!(
        png_path(Path::new(r"D:\Snips\v1.2 button.jpg")),
        PathBuf::from(r"D:\Snips\v1.2 button.png")
    );
}

// What the button coming up does with each Destination and "Save to default folder".

use std::time::Duration;

use screengauge_core::session::{
    Colors, Effect, Event, Held, Image, Input, Key, Message, Opening, Photo, Point, Rect, SaveTo,
    Scale, Session, ToolbarButton,
};
use screengauge_core::settings::{
    DefaultFolder, Folder, Options, Remembered, SnipDestination, Tool,
};

/// A 1080p monitor at 100%, white, with a blue button of 100 × 40 at columns 1000 to 1099 and rows 600 to 639.
fn button() -> Photo {
    let rect = Rect {
        left: 0,
        top: 0,
        width: 1920,
        height: 1080,
    };
    let mut bgra = [255, 255, 255, 0].repeat(1920 * 1080);
    for y in 600..640 {
        for x in 1000..1100 {
            let at = (y * 1920 + x) * 4;
            bgra[at..at + 3].copy_from_slice(&[235, 99, 37]);
        }
    }
    Photo {
        rect,
        scale: Scale::from_dpi(96).expect("100%"),
        colors: Colors::Exact,
        image: Image::new(1920, 1080, bgra).expect("the pixels fill the image"),
    }
}

fn at(x: i32, y: i32) -> Point {
    Point { x, y }
}

fn folder(path: &str) -> Folder {
    Folder::new(Path::new(path)).expect("an absolute path")
}

/// Snip active with `destination` and `default_folder`, the cursor at (900, 500), Save As never used yet.
fn open(destination: SnipDestination, default_folder: DefaultFolder) -> Session {
    open_remembering(destination, default_folder, None)
}

fn open_remembering(
    destination: SnipDestination,
    default_folder: DefaultFolder,
    last_save_as_folder: Option<Folder>,
) -> Session {
    Session::open(Opening {
        frozen_screen: vec![Some(button())],
        cursor: at(900, 500),
        options: Options {
            snip_destination: destination,
            default_folder,
            ..Options::default()
        },
        remembered: Remembered {
            last_tool: Tool::Snip,
            last_save_as_folder,
            ..Remembered::default()
        },
        second_click_until: None,
    })
    .expect("a frozen screen the session takes")
}

/// `input` with the cursor at `cursor`, nothing held, `milliseconds` after Windows started, at [`EVENING`].
fn event(cursor: Point, milliseconds: u64, input: Input) -> Event {
    Event {
        cursor,
        held: Held::default(),
        time: EVENING,
        uptime: Duration::from_millis(milliseconds),
        input,
    }
}

/// Drags around the button and lets go at (1200, 700), and, as the Windows layer does, has the clipboard take any copy
/// the release asks for (WPC-352): what the release and the answer return, in their order.
fn snip(session: &mut Session) -> Vec<Effect> {
    session.handle(event(at(900, 500), 1000, Input::Move));
    session.handle(event(at(900, 500), 1010, Input::Press));
    session.handle(event(at(1200, 700), 1020, Input::Move));
    let mut effects = session.handle(event(at(1200, 700), 1030, Input::Release));
    if effects
        .iter()
        .any(|effect| matches!(effect, Effect::CopyImage(_)))
    {
        effects.extend(session.handle(event(at(1200, 700), 1030, Input::Copied)));
    }
    effects
}

/// What the effects do, in their order, without the images' pixels: a wake only says when.
#[derive(Debug, PartialEq)]
enum Did {
    Copy,
    Write(SaveTo),
    OpenSaveAs {
        folder: Option<Folder>,
        file_name: String,
    },
    StepAside,
    ComeBack,
    Wake,
    Close,
    Remember(Option<Folder>),
}

fn did(effects: Vec<Effect>) -> Vec<Did> {
    effects
        .into_iter()
        .map(|effect| match effect {
            Effect::CopyImage(image) => {
                assert_eq!((image.width(), image.height()), (100, 40));
                Did::Copy
            }
            Effect::WritePng { to, image } => {
                assert_eq!((image.width(), image.height()), (100, 40));
                Did::Write(to)
            }
            Effect::OpenSaveAs { folder, file_name } => Did::OpenSaveAs { folder, file_name },
            Effect::StepAside => Did::StepAside,
            Effect::ComeBack => Did::ComeBack,
            Effect::WakeAt(_) => Did::Wake,
            Effect::Close => Did::Close,
            Effect::SaveRemembered(remembered) => Did::Remember(remembered.last_save_as_folder),
            other => panic!("not a Snip effect: {other:?}"),
        })
        .collect()
}

fn notice(session: &Session) -> Option<Message> {
    session.scene().notice.map(|notice| notice.message)
}

fn in_default_folder() -> SaveTo {
    SaveTo::DefaultFolder {
        folder: folder(r"D:\Snips"),
        names: FileNames::at(EVENING),
    }
}

fn suggested() -> Did {
    Did::OpenSaveAs {
        folder: None,
        file_name: String::from("screengauge-20261026-204112.png"),
    }
}

#[test]
fn clipboard_copies_with_no_window_whether_the_default_folder_is_on_or_off() {
    for default_folder in [
        DefaultFolder::Off(None),
        DefaultFolder::On(folder(r"D:\Snips")),
    ] {
        let mut session = open(SnipDestination::Clipboard, default_folder);
        assert_eq!(did(snip(&mut session)), [Did::Copy, Did::Wake]);
        assert_eq!(notice(&session), Some(Message::Copied));
    }
}

#[test]
fn file_with_the_default_folder_off_opens_save_as_with_the_automatic_name_and_copies_nothing() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    assert_eq!(did(snip(&mut session)), [suggested()]);
    assert_eq!(notice(&session), None);
}

#[test]
fn file_with_the_default_folder_on_writes_there_with_no_window() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    assert_eq!(did(snip(&mut session)), [Did::Write(in_default_folder())]);
}

#[test]
fn both_with_the_default_folder_off_copies_then_opens_save_as() {
    let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
    assert_eq!(did(snip(&mut session)), [Did::Copy, suggested()]);
    assert_eq!(notice(&session), None);
}

#[test]
fn both_with_the_default_folder_on_copies_then_writes_there() {
    let mut session = open(
        SnipDestination::Both,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    assert_eq!(
        did(snip(&mut session)),
        [Did::Copy, Did::Write(in_default_folder())]
    );
}

#[test]
fn a_folder_kept_while_the_default_folder_is_off_is_not_written_to() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::Off(Some(folder(r"D:\Snips"))),
    );
    assert_eq!(did(snip(&mut session)), [suggested()]);
}

// What comes after: the file written or not, Save As closing, and what Save As being open changes.

fn save_as_closed(session: &mut Session, chosen: Option<&str>) -> Vec<Effect> {
    let chosen = chosen.map(PathBuf::from);
    session.handle(event(at(960, 540), 5000, Input::SaveAsClosed(chosen)))
}

fn written(session: &mut Session, saved: bool) -> Vec<Effect> {
    let input = if saved { Input::Saved } else { Input::NotSaved };
    session.handle(event(at(960, 540), 5010, input))
}

/// Closes the tools with Esc and says what the remembered state keeps, as the settings file would.
fn remembered_folder_at_closing(session: &mut Session) -> Option<Option<Folder>> {
    did(session.handle(event(at(960, 540), 9000, Input::KeyDown(Key::Esc))))
        .into_iter()
        .find_map(|did| match did {
            Did::Remember(folder) => Some(folder),
            _ => None,
        })
}

fn chosen(path: &str) -> Did {
    Did::Write(SaveTo::Chosen(PathBuf::from(path)))
}

#[test]
fn the_default_folder_says_saved_once_the_file_is_written() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    assert_eq!(notice(&session), None);
    assert_eq!(did(written(&mut session, true)), [Did::Wake]);
    assert_eq!(notice(&session), Some(Message::Saved));
}

#[test]
fn both_says_copied_and_saved() {
    let mut session = open(
        SnipDestination::Both,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    written(&mut session, true);
    assert_eq!(notice(&session), Some(Message::CopiedAndSaved));
}

#[test]
fn a_file_that_couldnt_be_written_says_so_and_the_tools_stay_open() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    let effects = did(written(&mut session, false));
    assert!(!effects.contains(&Did::Close), "{effects:?}");
    assert_eq!(notice(&session), Some(Message::CouldntSave));
    assert_eq!(Message::CouldntSave.text(), "Couldn't save.");
    // The tools go on, with the options as they were: the next drag writes to the same folder again.
    assert_eq!(did(snip(&mut session)), [Did::Write(in_default_folder())]);
}

#[test]
fn with_both_a_failed_file_says_the_image_was_copied() {
    let mut session = open(
        SnipDestination::Both,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    written(&mut session, false);
    assert_eq!(notice(&session), Some(Message::CouldntSaveButCopied));
    assert_eq!(
        Message::CouldntSaveButCopied.text(),
        "Couldn't save. The image was copied."
    );
}

#[test]
fn save_as_writes_the_image_where_the_user_chose_and_says_saved_after_it_closes() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(
        did(save_as_closed(
            &mut session,
            Some(r"E:\Work\login button.png")
        )),
        [chosen(r"E:\Work\login button.png")]
    );
    assert_eq!(notice(&session), None);
    written(&mut session, true);
    let shown = session.scene().notice.copied().expect("a notice");
    assert_eq!(shown.message, Message::Saved);
    // Where the cursor is as Save As closes, not where the drag ended.
    assert_eq!(shown.at, at(960, 540));
}

#[test]
fn a_name_chosen_in_save_as_always_ends_with_png() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(
        did(save_as_closed(
            &mut session,
            Some(r"E:\Work\login button.jpg")
        )),
        [chosen(r"E:\Work\login button.png")]
    );
}

#[test]
fn cancelling_save_as_with_file_makes_nothing_and_shows_nothing() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(did(save_as_closed(&mut session, None)), []);
    assert_eq!(notice(&session), None);
}

#[test]
fn cancelling_save_as_with_both_shows_copied() {
    let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(did(save_as_closed(&mut session, None)), [Did::Wake]);
    assert_eq!(notice(&session), Some(Message::Copied));
}

#[test]
fn save_as_that_couldnt_open_says_couldnt_save_and_the_tools_go_on() {
    for (destination, message) in [
        (SnipDestination::File, Message::CouldntSave),
        (SnipDestination::Both, Message::CouldntSaveButCopied),
    ] {
        let mut session = open(destination, DefaultFolder::Off(None));
        snip(&mut session);
        let effects = did(session.handle(event(at(960, 540), 2000, Input::SaveAsFailed)));
        assert_eq!(effects, [Did::Wake]);
        assert_eq!(notice(&session), Some(message));
        assert_eq!(
            did(snip(&mut session)).len(),
            usize::from(copied(destination)) + 1
        );
    }
}

fn copied(destination: SnipDestination) -> bool {
    destination == SnipDestination::Both
}

#[test]
fn save_as_with_both_says_copied_and_saved_or_that_the_image_was_copied() {
    for (saved, message) in [
        (true, Message::CopiedAndSaved),
        (false, Message::CouldntSaveButCopied),
    ] {
        let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
        snip(&mut session);
        save_as_closed(&mut session, Some(r"E:\Work\button.png"));
        written(&mut session, saved);
        assert_eq!(notice(&session), Some(message));
    }
}

#[test]
fn the_folder_save_as_wrote_to_is_where_it_opens_next_and_is_kept_at_closing() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    save_as_closed(&mut session, Some(r"E:\Work\button.png"));
    written(&mut session, true);
    assert_eq!(
        did(snip(&mut session)),
        [Did::OpenSaveAs {
            folder: Some(folder(r"E:\Work")),
            file_name: String::from("screengauge-20261026-204112.png"),
        }]
    );
    save_as_closed(&mut session, None);
    assert_eq!(
        remembered_folder_at_closing(&mut session),
        Some(Some(folder(r"E:\Work")))
    );
}

#[test]
fn save_as_opens_in_the_folder_remembered_from_an_earlier_call() {
    let mut session = open_remembering(
        SnipDestination::File,
        DefaultFolder::Off(None),
        Some(folder(r"E:\Work")),
    );
    assert_eq!(
        did(snip(&mut session)),
        [Did::OpenSaveAs {
            folder: Some(folder(r"E:\Work")),
            file_name: String::from("screengauge-20261026-204112.png"),
        }]
    );
}

#[test]
fn a_folder_save_as_couldnt_write_to_is_not_remembered() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    save_as_closed(&mut session, Some(r"\\server\offline\button.png"));
    written(&mut session, false);
    assert_eq!(remembered_folder_at_closing(&mut session), None);
}

#[test]
fn writing_to_the_default_folder_never_changes_where_save_as_opens() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    written(&mut session, true);
    assert_eq!(remembered_folder_at_closing(&mut session), None);
}

#[test]
fn with_save_as_open_no_other_drag_starts_and_esc_and_the_toolbar_do_nothing() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(did(snip(&mut session)), []);
    assert_eq!(session.scene().snip_area, None);
    for input in [
        Input::KeyDown(Key::Esc),
        Input::KeyDown(Key::Digit1),
        Input::Toolbar(ToolbarButton::Close),
        Input::CloseAsked,
    ] {
        assert_eq!(did(session.handle(event(at(960, 540), 3000, input))), []);
    }
    // Save As still has the image: choosing a place writes it.
    assert_eq!(
        did(save_as_closed(&mut session, Some(r"E:\Work\button.png"))),
        [chosen(r"E:\Work\button.png")]
    );
}

#[test]
fn another_program_in_front_of_save_as_sends_both_behind_it_and_their_button_brings_them_back() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    let other_program = event(at(960, 540), 3000, Input::OtherProgramInFront);
    assert_eq!(did(session.handle(other_program.clone())), [Did::StepAside]);
    // Going from one program to another changes nothing more.
    assert_eq!(did(session.handle(other_program)), []);
    assert_eq!(
        did(session.handle(event(at(960, 540), 4000, Input::BackInFront))),
        [Did::ComeBack]
    );
    // Nothing was lost: the image is written where the user then chooses.
    assert_eq!(
        did(save_as_closed(&mut session, Some(r"E:\Work\button.png"))),
        [chosen(r"E:\Work\button.png")]
    );
}

#[test]
fn the_global_shortcut_with_save_as_open_brings_it_back_and_never_closes_the_tools() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    let shortcut = event(at(960, 540), 3000, Input::GlobalShortcut);
    assert_eq!(did(session.handle(shortcut.clone())), []);
    session.handle(event(at(960, 540), 3100, Input::OtherProgramInFront));
    assert_eq!(did(session.handle(shortcut)), [Did::ComeBack]);
}

#[test]
fn calling_screengauge_from_the_tray_or_start_with_save_as_aside_brings_it_back() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    let call = event(at(960, 540), 3000, Input::Called);
    assert_eq!(did(session.handle(call.clone())), []);
    session.handle(event(at(960, 540), 3100, Input::OtherProgramInFront));
    assert_eq!(did(session.handle(call)), [Did::ComeBack]);
}

#[test]
fn after_save_as_another_program_in_front_closes_the_tools_again() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    save_as_closed(&mut session, None);
    assert_eq!(
        did(session.handle(event(at(960, 540), 6000, Input::OtherProgramInFront))),
        [Did::Close]
    );
}

#[test]
fn monitors_changing_with_save_as_open_close_the_tools_only_after_the_file_is_written() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    assert_eq!(
        did(session.handle(event(at(960, 540), 3000, Input::MonitorsChanged))),
        []
    );
    assert_eq!(
        did(save_as_closed(&mut session, Some(r"E:\Work\button.png"))),
        [chosen(r"E:\Work\button.png")]
    );
    // The folder is kept as the tools close, since the file was written there.
    assert_eq!(
        did(written(&mut session, true)),
        [Did::Close, Did::Remember(Some(folder(r"E:\Work")))]
    );
}

#[test]
fn monitors_changing_with_save_as_open_then_cancelled_close_the_tools() {
    let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
    snip(&mut session);
    session.handle(event(at(960, 540), 3000, Input::MonitorsChanged));
    assert_eq!(did(save_as_closed(&mut session, None)), [Did::Close]);
}

#[test]
fn monitors_changing_with_save_as_aside_then_cancelled_bring_the_frozen_screen_back_before_closing()
{
    let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
    snip(&mut session);
    session.handle(event(at(960, 540), 3000, Input::OtherProgramInFront));
    session.handle(event(at(960, 540), 3010, Input::MonitorsChanged));
    assert_eq!(
        did(save_as_closed(&mut session, None)),
        [Did::ComeBack, Did::Close]
    );
}

#[test]
fn monitors_changing_with_save_as_that_couldnt_open_close_the_tools_without_a_notice() {
    let mut session = open(SnipDestination::Both, DefaultFolder::Off(None));
    snip(&mut session);
    session.handle(event(at(960, 540), 3000, Input::MonitorsChanged));
    assert_eq!(
        did(session.handle(event(at(960, 540), 3010, Input::SaveAsFailed))),
        [Did::Close]
    );
    assert_eq!(notice(&session), None);
}

#[test]
fn exiting_with_save_as_aside_closes_the_tools_and_keeps_the_folder_this_call_wrote_to() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    snip(&mut session);
    save_as_closed(&mut session, Some(r"E:\Work\button.png"));
    written(&mut session, true);
    // Save As again, then another program in front: the tray icon is in reach, and its menu's Exit is chosen.
    snip(&mut session);
    session.handle(event(at(960, 540), 6000, Input::OtherProgramInFront));
    assert_eq!(
        did(session.handle(event(at(960, 540), 7000, Input::Exit))),
        [Did::Close, Did::Remember(Some(folder(r"E:\Work")))]
    );
    // Save As, cancelled as ScreenGauge exits, finds the tools closed.
    assert_eq!(did(save_as_closed(&mut session, None)), []);
}

#[test]
fn exiting_while_the_default_folder_is_written_closes_the_tools() {
    let mut session = open(
        SnipDestination::File,
        DefaultFolder::On(folder(r"D:\Snips")),
    );
    snip(&mut session);
    assert_eq!(
        did(session.handle(event(at(960, 540), 3000, Input::Exit))),
        [Did::Close]
    );
    assert_eq!(did(written(&mut session, true)), []);
}

#[test]
fn exiting_after_the_tools_closed_does_nothing() {
    let mut session = open(SnipDestination::File, DefaultFolder::Off(None));
    session.handle(event(at(960, 540), 3000, Input::KeyDown(Key::Esc)));
    assert_eq!(
        did(session.handle(event(at(960, 540), 4000, Input::Exit))),
        []
    );
}
