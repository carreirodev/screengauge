//! The subset of SVG path data the toolbar's icons are written in (the prototype's `<path d="...">`, WPC-279): moveto,
//! lineto, horizontal and vertical lineto, elliptical arcs and closepath, each absolute or relative.
//! Parsing needs no Windows, so it is tested without a graphics card; `toolbar_paint` turns the figures it returns
//! into Direct2D geometry.

use std::fmt;

/// A point in the icon's own units (the SVG's 24 by 24 view box).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// An elliptical arc as SVG writes it: Direct2D's arc segment takes the same parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Arc {
    /// The ellipse's two radii, never negative or zero: an arc with a zero radius is a line.
    pub radii: Pt,
    /// The ellipse's turn from the x axis, in degrees.
    pub rotation: f32,
    /// Whether the longer way around the ellipse is taken.
    pub large: bool,
    /// Whether the arc is drawn in the direction of increasing angle, which is clockwise on a screen.
    pub sweep: bool,
    pub to: Pt,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Line(Pt),
    Arc(Arc),
}

/// One subpath: a start and what leads on from it. An open figure ends where its last segment does.
#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub start: Pt,
    pub segments: Vec<Segment>,
    /// Whether a closepath ended it, so its last point joins its first.
    pub closed: bool,
}

/// What is wrong with a path, and the byte of the text where it was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub kind: ErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// A path starts with a moveto.
    MissingMoveto,
    /// A letter that is no command of the subset (the icons draw no curve but arcs).
    UnknownCommand(char),
    /// A number was due and something else, or the end of the text, came.
    ExpectedNumber,
    /// An arc's flag is the single character 0 or 1.
    ExpectedFlag,
    /// The text of a number is not a finite number.
    NumberOutOfRange,
    /// Numbers after a closepath, which takes none.
    NumberAfterClose,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            ErrorKind::MissingMoveto => "the path does not start with a moveto".to_owned(),
            ErrorKind::UnknownCommand(c) => format!("`{c}` is not a path command of the icons"),
            ErrorKind::ExpectedNumber => "a number was expected".to_owned(),
            ErrorKind::ExpectedFlag => "an arc's flag must be 0 or 1".to_owned(),
            ErrorKind::NumberOutOfRange => "a number is not finite".to_owned(),
            ErrorKind::NumberAfterClose => "numbers follow a closepath".to_owned(),
        };
        write!(f, "path data, byte {}: {what}", self.at)
    }
}

impl std::error::Error for ParseError {}

/// Reads path data from a text, one character at a time.
struct Reader<'a> {
    text: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn error(&self, kind: ErrorKind) -> ParseError {
        ParseError { at: self.at, kind }
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    /// Skips white space, and one comma if there is one: what SVG allows between two numbers.
    fn skip_separators(&mut self) {
        self.skip_space();
        if self.peek() == Some(b',') {
            self.at += 1;
            self.skip_space();
        }
    }

    fn skip_space(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    /// Whether a number follows, after the separators: the sign of one, a digit or a point.
    fn number_follows(&mut self) -> bool {
        self.skip_separators();
        self.peek()
            .is_some_and(|c| c.is_ascii_digit() || matches!(c, b'+' | b'-' | b'.'))
    }

    fn digits(&mut self) -> usize {
        let from = self.at;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.at += 1;
        }
        self.at - from
    }

    /// A number: an optional sign, and digits with an optional point. A second point, or a sign, ends it, so `1.5-1` and
    /// `.1.1` are two numbers each.
    fn number(&mut self) -> Result<f32, ParseError> {
        self.skip_separators();
        let from = self.at;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let whole = self.digits();
        let mut fraction = 0;
        if self.peek() == Some(b'.') {
            self.at += 1;
            fraction = self.digits();
        }
        if whole + fraction == 0 {
            self.at = from;
            return Err(self.error(ErrorKind::ExpectedNumber));
        }
        // Only ASCII digits, signs and points were taken, so the slice is valid text.
        let text = std::str::from_utf8(&self.text[from..self.at])
            .map_err(|_| self.error(ErrorKind::ExpectedNumber))?;
        let value: f32 = text.parse().map_err(|_| ParseError {
            at: from,
            kind: ErrorKind::ExpectedNumber,
        })?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(ParseError {
                at: from,
                kind: ErrorKind::NumberOutOfRange,
            })
        }
    }

    /// An arc's flag: one character, 0 or 1, with no separator needed after it (`a1 1 0 011 1`).
    fn flag(&mut self) -> Result<bool, ParseError> {
        self.skip_separators();
        let flag = match self.peek() {
            Some(b'0') => false,
            Some(b'1') => true,
            _ => return Err(self.error(ErrorKind::ExpectedFlag)),
        };
        self.at += 1;
        Ok(flag)
    }

    fn point(&mut self) -> Result<Pt, ParseError> {
        Ok(Pt {
            x: self.number()?,
            y: self.number()?,
        })
    }
}

/// Where the path is: the current point, the start of the subpath it is in, and the figure being built.
struct Pen {
    current: Pt,
    start: Pt,
    figure: Option<Figure>,
    done: Vec<Figure>,
}

impl Pen {
    /// A figure with at least one segment is kept; a lone moveto draws nothing.
    fn finish(&mut self, closed: bool) {
        if let Some(mut figure) = self.figure.take() {
            figure.closed = closed;
            if !figure.segments.is_empty() {
                self.done.push(figure);
            }
        }
    }

    fn move_to(&mut self, to: Pt) {
        self.finish(false);
        self.current = to;
        self.start = to;
    }

    /// Adds `segment`, ending at `to`. After a closepath the next segment begins a new figure at the old one's start,
    /// which is where the current point went.
    fn push(&mut self, segment: Segment, to: Pt) {
        let start = self.current;
        let figure = self.figure.get_or_insert_with(|| Figure {
            start,
            segments: Vec::new(),
            closed: false,
        });
        figure.segments.push(segment);
        self.current = to;
    }

    fn close(&mut self) {
        self.finish(true);
        self.current = self.start;
    }
}

/// The figures of `d`, in SVG path data. A path with no drawable figure gives none.
pub fn parse(d: &str) -> Result<Vec<Figure>, ParseError> {
    let mut reader = Reader {
        text: d.as_bytes(),
        at: 0,
    };
    let mut pen = Pen {
        current: Pt { x: 0.0, y: 0.0 },
        start: Pt { x: 0.0, y: 0.0 },
        figure: None,
        done: Vec::new(),
    };
    reader.skip_space();
    if !matches!(reader.peek(), Some(b'M' | b'm')) {
        return Err(reader.error(ErrorKind::MissingMoveto));
    }
    loop {
        reader.skip_space();
        let Some(byte) = reader.peek() else {
            break;
        };
        let command = char::from(byte);
        if !command.is_ascii_alphabetic() {
            return Err(reader.error(ErrorKind::UnknownCommand(command)));
        }
        reader.at += 1;
        if matches!(command, 'Z' | 'z') {
            pen.close();
            if reader.number_follows() {
                return Err(reader.error(ErrorKind::NumberAfterClose));
            }
            continue;
        }
        if !"MmLlHhVvAa".contains(command) {
            return Err(ParseError {
                at: reader.at - 1,
                kind: ErrorKind::UnknownCommand(command),
            });
        }
        // The command's arguments, and then again for as long as numbers follow: the command repeats, and a moveto's
        // repeats are linetos.
        let mut repeat = command;
        loop {
            run(&mut reader, &mut pen, repeat)?;
            if !reader.number_follows() {
                break;
            }
            repeat = match repeat {
                'M' => 'L',
                'm' => 'l',
                other => other,
            };
        }
    }
    pen.finish(false);
    Ok(pen.done)
}

/// One set of arguments of `command`, carried out.
fn run(reader: &mut Reader<'_>, pen: &mut Pen, command: char) -> Result<(), ParseError> {
    let relative = command.is_ascii_lowercase();
    // Where a relative coordinate is measured from.
    let origin = if relative {
        pen.current
    } else {
        Pt { x: 0.0, y: 0.0 }
    };
    let place = |p: Pt| Pt {
        x: origin.x + p.x,
        y: origin.y + p.y,
    };
    match command.to_ascii_uppercase() {
        'M' => {
            let to = place(reader.point()?);
            pen.move_to(to);
        }
        'L' => {
            let to = place(reader.point()?);
            pen.push(Segment::Line(to), to);
        }
        'H' => {
            let x = reader.number()?;
            let to = Pt {
                x: origin.x + x,
                y: pen.current.y,
            };
            pen.push(Segment::Line(to), to);
        }
        'V' => {
            let y = reader.number()?;
            let to = Pt {
                x: pen.current.x,
                y: origin.y + y,
            };
            pen.push(Segment::Line(to), to);
        }
        _ => {
            let radii = reader.point()?;
            let rotation = reader.number()?;
            let large = reader.flag()?;
            let sweep = reader.flag()?;
            let to = place(reader.point()?);
            arc(pen, radii, rotation, large, sweep, to);
        }
    }
    Ok(())
}

/// Adds an arc the way SVG says to: none at all when it ends where it starts, and a straight line when either radius
/// is zero. A negative radius is its absolute value.
fn arc(pen: &mut Pen, radii: Pt, rotation: f32, large: bool, sweep: bool, to: Pt) {
    if to == pen.current {
        return;
    }
    let radii = Pt {
        x: radii.x.abs(),
        y: radii.y.abs(),
    };
    if radii.x == 0.0 || radii.y == 0.0 {
        pen.push(Segment::Line(to), to);
        return;
    }
    let arc = Arc {
        radii,
        rotation,
        large,
        sweep,
        to,
    };
    pen.push(Segment::Arc(arc), to);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> Pt {
        Pt { x, y }
    }

    /// The one figure of `d`.
    fn only(d: &str) -> Figure {
        let mut figures = parse(d).expect("a path");
        assert_eq!(figures.len(), 1, "{d}");
        figures.remove(0)
    }

    fn lines(figure: &Figure) -> Vec<Pt> {
        figure
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Line(to) => *to,
                other => panic!("not a line: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn absolute_moveto_and_lineto_give_an_open_figure() {
        let figure = only("M4 9L20 9L20 15");
        assert_eq!(figure.start, pt(4.0, 9.0));
        assert_eq!(lines(&figure), [pt(20.0, 9.0), pt(20.0, 15.0)]);
        assert!(!figure.closed);
    }

    #[test]
    fn relative_commands_measure_from_the_current_point() {
        let figure = only("M10 10l5 -2l-1 6");
        assert_eq!(lines(&figure), [pt(15.0, 8.0), pt(14.0, 14.0)]);
    }

    #[test]
    fn horizontal_and_vertical_lines_keep_the_other_coordinate() {
        let figure = only("M4 8h3V3v5H0h-2");
        assert_eq!(
            lines(&figure),
            [
                pt(7.0, 8.0),
                pt(7.0, 3.0),
                pt(7.0, 8.0),
                pt(0.0, 8.0),
                pt(-2.0, 8.0)
            ]
        );
    }

    #[test]
    fn the_first_relative_moveto_is_absolute() {
        let figure = only("m3 4l1 1");
        assert_eq!(figure.start, pt(3.0, 4.0));
        assert_eq!(lines(&figure), [pt(4.0, 5.0)]);
    }

    #[test]
    fn a_moveto_with_more_pairs_goes_on_with_linetos() {
        let figure = only("M0 0 10 10 20 0");
        assert_eq!(lines(&figure), [pt(10.0, 10.0), pt(20.0, 0.0)]);
        // A relative moveto repeats as a relative lineto.
        let figure = only("m1 1 2 2 3 3");
        assert_eq!(figure.start, pt(1.0, 1.0));
        assert_eq!(lines(&figure), [pt(3.0, 3.0), pt(6.0, 6.0)]);
    }

    #[test]
    fn a_command_repeats_while_numbers_follow() {
        let figure = only("M0 0h1 2 3v1 1");
        assert_eq!(
            lines(&figure),
            [
                pt(1.0, 0.0),
                pt(3.0, 0.0),
                pt(6.0, 0.0),
                pt(6.0, 1.0),
                pt(6.0, 2.0)
            ]
        );
    }

    #[test]
    fn a_second_moveto_starts_a_second_figure() {
        let figures = parse("M6.5 8v3M10.5 8v4.5").expect("a path");
        assert_eq!(figures.len(), 2);
        assert_eq!(figures[0].start, pt(6.5, 8.0));
        assert_eq!(figures[1].start, pt(10.5, 8.0));
        assert_eq!(lines(&figures[1]), [pt(10.5, 12.5)]);
    }

    #[test]
    fn numbers_run_together_where_the_syntax_allows() {
        // A sign ends a number, and so does a second point.
        let figure = only("M0 0l1.5-1l.1.1l-.1-.1l.3 -.2");
        let wanted = [pt(1.5, -1.0), pt(1.6, -0.9), pt(1.5, -1.0), pt(1.8, -1.2)];
        let got = lines(&figure);
        assert_eq!(got.len(), wanted.len());
        for (got, wanted) in got.iter().zip(wanted) {
            assert!(
                (got.x - wanted.x).abs() < 1e-5 && (got.y - wanted.y).abs() < 1e-5,
                "{got:?} {wanted:?}"
            );
        }
    }

    #[test]
    fn commas_white_space_and_signs_separate_and_start_numbers() {
        let figure = only(" M +1,2\n L\t3 ,4 10 -2.5 ");
        assert_eq!(figure.start, pt(1.0, 2.0));
        assert_eq!(lines(&figure), [pt(3.0, 4.0), pt(10.0, -2.5)]);
    }

    #[test]
    fn an_arc_keeps_its_radii_rotation_flags_and_end() {
        let figure = only("M10 10a2 3 30 1 0 4 0");
        assert_eq!(
            figure.segments,
            [Segment::Arc(Arc {
                radii: pt(2.0, 3.0),
                rotation: 30.0,
                large: true,
                sweep: false,
                to: pt(14.0, 10.0),
            })]
        );
        let figure = only("M10 10A2 2 0 0 1 12 12");
        let Segment::Arc(arc) = figure.segments[0] else {
            panic!("an arc");
        };
        assert!(!arc.large && arc.sweep);
        assert_eq!(arc.to, pt(12.0, 12.0));
    }

    #[test]
    fn an_arcs_flags_need_no_separator_after_them() {
        let figure = only("M0 0a1 1 0 011 1");
        let Segment::Arc(arc) = figure.segments[0] else {
            panic!("an arc");
        };
        assert!(!arc.large && arc.sweep);
        assert_eq!(arc.to, pt(1.0, 1.0));
        // And the number after the flags may be glued to them by its sign, as in the gear.
        let figure = only("M5 5a1.7 1.7 0 0 0-1.8-.3");
        let Segment::Arc(arc) = figure.segments[0] else {
            panic!("an arc");
        };
        assert!(!arc.large && !arc.sweep);
        assert!((arc.to.x - 3.2).abs() < 1e-5 && (arc.to.y - 4.7).abs() < 1e-5);
    }

    #[test]
    fn an_arc_repeats_with_its_own_flags_each_time() {
        let figure = only("M0 0a1 1 0 0 0 1 1 1 1 0 1 1 1 1");
        let flags: Vec<(bool, bool)> = figure
            .segments
            .iter()
            .map(|segment| match segment {
                Segment::Arc(arc) => (arc.large, arc.sweep),
                other => panic!("not an arc: {other:?}"),
            })
            .collect();
        assert_eq!(flags, [(false, false), (true, true)]);
    }

    #[test]
    fn an_arc_with_no_radius_is_a_line_and_one_that_goes_nowhere_is_nothing() {
        let figure = only("M0 0a0 5 0 0 0 3 4");
        assert_eq!(lines(&figure), [pt(3.0, 4.0)]);
        let figure = only("M0 0a5 5 0 0 0 0 0l1 1");
        assert_eq!(lines(&figure), [pt(1.0, 1.0)]);
        // A negative radius counts as its absolute value.
        let figure = only("M0 0a-2 -3 0 0 0 3 4");
        let Segment::Arc(arc) = figure.segments[0] else {
            panic!("an arc");
        };
        assert_eq!(arc.radii, pt(2.0, 3.0));
    }

    #[test]
    fn a_closepath_closes_the_figure_and_goes_back_to_its_start() {
        let figure = only("M4 8h3l2-3z");
        assert!(figure.closed);
        // The closing side belongs to the closed figure, and is not a segment of its own.
        assert_eq!(figure.segments.len(), 2);
        // Absolute and relative closepaths are the same.
        assert!(only("M0 0L1 0L1 1Z").closed);
    }

    #[test]
    fn a_command_after_a_closepath_starts_a_new_figure_where_the_old_one_began() {
        let figures = parse("M2 2h4v4zh1").expect("a path");
        assert_eq!(figures.len(), 2);
        assert!(figures[0].closed);
        assert_eq!(figures[1].start, pt(2.0, 2.0));
        assert_eq!(lines(&figures[1]), [pt(3.0, 2.0)]);
        assert!(!figures[1].closed);
        // The same with a relative moveto: it is measured from the start of the old figure.
        let figures = parse("M2 2h4v4zm1 1h1").expect("a path");
        assert_eq!(figures[1].start, pt(3.0, 3.0));
    }

    #[test]
    fn a_lone_moveto_or_an_empty_figure_draws_nothing() {
        assert_eq!(parse("M1 1").expect("a path"), []);
        assert_eq!(parse("M1 1M2 2L3 3").expect("a path").len(), 1);
        assert_eq!(parse("M1 1z").expect("a path"), []);
    }

    #[test]
    fn the_cameras_outline_is_one_closed_figure_of_seven_lines() {
        let figure = only("M4 8h3l2-3h6l2 3h3v11H4z");
        assert!(figure.closed);
        assert_eq!(figure.start, pt(4.0, 8.0));
        assert_eq!(
            lines(&figure),
            [
                pt(7.0, 8.0),
                pt(9.0, 5.0),
                pt(15.0, 5.0),
                pt(17.0, 8.0),
                pt(20.0, 8.0),
                pt(20.0, 19.0),
                pt(4.0, 19.0),
            ]
        );
    }

    /// The gear's outline, as the prototype writes it.
    const GEAR: &str = "M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z";

    #[test]
    fn the_gear_is_one_closed_figure_of_24_arcs_and_18_lines() {
        // Counted with a separate tokenizer, command by command: 24 arcs (the 39 commands carry several each), 18
        // lines of the l, h, v, H and V commands, one moveto and one closepath.
        let figure = only(GEAR);
        assert!(figure.closed);
        assert_eq!(figure.start, pt(19.4, 15.0));
        let arcs = figure
            .segments
            .iter()
            .filter(|segment| matches!(segment, Segment::Arc(_)))
            .count();
        assert_eq!((arcs, figure.segments.len() - arcs), (24, 18));
    }

    #[test]
    fn the_gear_stays_inside_its_view_box_and_ends_where_it_began() {
        let figure = only(GEAR);
        let mut end = figure.start;
        for segment in &figure.segments {
            end = match segment {
                Segment::Line(to) => *to,
                Segment::Arc(arc) => arc.to,
            };
            assert!(
                (0.0..=24.0).contains(&end.x) && (0.0..=24.0).contains(&end.y),
                "{end:?}"
            );
        }
        // The last arc comes back to the first point, to the precision of the path's decimals.
        assert!(
            (end.x - 19.4).abs() < 0.05 && (end.y - 15.0).abs() < 0.05,
            "{end:?}"
        );
    }

    #[test]
    fn a_path_must_start_with_a_moveto() {
        assert_eq!(parse("").unwrap_err().kind, ErrorKind::MissingMoveto);
        assert_eq!(parse("   ").unwrap_err().kind, ErrorKind::MissingMoveto);
        assert_eq!(parse("L1 1").unwrap_err().kind, ErrorKind::MissingMoveto);
        assert_eq!(parse("5 5").unwrap_err().kind, ErrorKind::MissingMoveto);
    }

    #[test]
    fn a_command_outside_the_subset_is_an_error() {
        for d in [
            "M0 0S1 1 2 2",
            "M0 0Q1 1 2 2",
            "M0 0T1 1",
            "M0 0C1 2 3 4 5 6",
            "M0 0c1 2 3 4 5 6",
            "M0 0x",
            "M0 0L1 1e1",
        ] {
            assert!(
                matches!(parse(d).unwrap_err().kind, ErrorKind::UnknownCommand(_)),
                "{d}"
            );
        }
        assert_eq!(
            parse("M0 0S1 1 2 2").unwrap_err(),
            ParseError {
                at: 4,
                kind: ErrorKind::UnknownCommand('S')
            }
        );
    }

    #[test]
    fn missing_numbers_are_errors_not_panics() {
        for d in [
            "M",
            "M1",
            "M1 ",
            "M0 0L",
            "M0 0L1",
            "M0 0h",
            "M0 0a1 1 0 0 0 1",
            "M0 0L1 x",
            "M0 0L-",
            "M0 0L. 1",
        ] {
            assert!(parse(d).is_err(), "{d}");
        }
    }

    #[test]
    fn a_bad_flag_is_an_error() {
        for d in [
            "M0 0a1 1 0 2 0 1 1",
            "M0 0a1 1 0 0 x 1 1",
            "M0 0a1 1 0 0",
            "M0 0a1 1 0 .5 0 1 1",
        ] {
            assert_eq!(parse(d).unwrap_err().kind, ErrorKind::ExpectedFlag, "{d}");
        }
    }

    #[test]
    fn numbers_after_a_closepath_are_an_error() {
        assert_eq!(
            parse("M0 0L1 1z 5").unwrap_err().kind,
            ErrorKind::NumberAfterClose
        );
    }

    #[test]
    fn a_number_too_large_to_hold_is_an_error() {
        // Far past what an f32 holds.
        let huge = "9".repeat(60);
        assert_eq!(
            parse(&format!("M{huge} 0L1 1")).unwrap_err().kind,
            ErrorKind::NumberOutOfRange
        );
    }

    #[test]
    fn an_error_says_where_it_was_found() {
        let error = parse("M0 0L1 1H").unwrap_err();
        assert_eq!(error.at, 9);
        assert!(error.to_string().contains("byte 9"), "{error}");
    }

    #[test]
    fn text_that_is_not_ascii_is_an_error_not_a_panic() {
        assert!(parse("M0 0L1 \u{e9}").is_err());
        assert!(parse("\u{e9}").is_err());
    }
}
