//! Where each part of Settings goes, top to bottom, in logical pixels: the four blocks' headings and cards, in the
//! spec's order, and the footer at the bottom (v1 spec WPC-296, Settings). The sizes are the prototype's stylesheet
//! (`prototype/measurement-overlay`, `#settings`), measured in a browser for WPC-319 without the language row, which the
//! v1 has no more, and with the Snip's folder note shown, as it is with Destination at its default, Clipboard:
//!
//! | part | measured | here |
//! | -- | -- | -- |
//! | `.body`'s padding, top and bottom | 12 | 12 |
//! | room between two blocks (`gap`) | 10 | 10 |
//! | a heading (`h5`, 10.5 tall, and its margin of 5) | 15.5 | 16 |
//! | General's card (Global shortcut, Start with Windows, Theme) | 128 | 128 |
//! | Measurements' card (two rows with a note each) | 87.69 | 88 |
//! | Inspect's card (Color format) | 43.33 | 43 |
//! | Snip's card (Destination, Save to default folder, the folder's line and its note) | 145.28 | 145 |
//! | the footer (`.sfoot`) | 36.26 | 36 |
//!
//! The page measured 556.56 tall; in whole pixels it is 558. The user doesn't change the window's height, and the
//! blocks' rows go inside these cards: Measurements' and Inspect's from WPC-320, the Snip's from WPC-322, the global
//! shortcut's from WPC-323, the others with their own tickets. The Snip's card is this tall with its note, as measured;
//! without it the card is shorter (`snip_card::fitted`) and the window stays as it is. Only a message under a row's
//! label makes the page taller, while it shows: the row grows to hold it, and its card, the blocks below and the window
//! with it ([`Grown`], WPC-323).

/// The page's width.
pub const WIDTH: u32 = 560;

/// The room left and right of the cards and of the footer's texts (`.body` and `.sfoot`'s padding).
pub const SIDE: u32 = 16;

/// Above the first heading and below the last card (`.body`'s padding).
const PADDING: u32 = 12;

/// Between one block's card and the next block's heading (`.body`'s `gap`).
const GAP: u32 = 10;

/// A heading's room, its text's line and the margin below it.
const HEADING: u32 = 16;

/// A heading's line (`font: 700 10.5px/1`): the text is 10.5 tall, and so is its line.
pub const HEADING_LINE: f32 = 10.5;

/// How far a heading's text is in from the card's left edge (`margin-left`).
pub const HEADING_INDENT: u32 = 4;

/// The cards' corners (`.scard`'s `border-radius`).
pub const CARD_RADIUS: u32 = 12;

/// The footer's height, its 1-pixel line on top included.
const FOOTER: u32 = 36;

/// Below the footer's line, the room above its text (`.sfoot`'s padding-top).
pub const FOOTER_ABOVE: u32 = 9;

/// The footer's texts' size, and their line (`font-size: 12px` under `#settings`' line-height of 1.3).
pub const FOOTER_TEXT: u32 = 12;
pub const FOOTER_LINE: f32 = 12.0 * 1.3;

/// Between the footer's texts on the left (`.sfoot`'s `gap`).
pub const FOOTER_GAP: u32 = 7;

/// The blocks of options, in the order the spec gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    General,
    Measurements,
    Inspect,
    Snip,
}

impl Block {
    /// The heading, in capitals as the stylesheet writes it (`text-transform: uppercase`).
    pub fn heading(self) -> &'static str {
        match self {
            Block::General => "GENERAL",
            Block::Measurements => "MEASUREMENTS",
            Block::Inspect => "INSPECT",
            Block::Snip => "SNIP",
        }
    }

    /// The card's height, with the rows the spec gives the block.
    fn card(self) -> u32 {
        match self {
            Block::General => 128,
            Block::Measurements => 88,
            Block::Inspect => 43,
            Block::Snip => 145,
        }
    }
}

/// The blocks, top to bottom.
pub const BLOCKS: [Block; 4] = [
    Block::General,
    Block::Measurements,
    Block::Inspect,
    Block::Snip,
];

/// From `top` down to `bottom`, the last row not included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub top: u32,
    pub bottom: u32,
}

/// One block: where its heading's text starts, and its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub block: Block,
    pub heading: u32,
    pub card: Span,
}

/// The whole page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub blocks: [Placed; 4],
    /// From the footer's line down to the bottom of the page.
    pub footer: Span,
    pub height: u32,
}

/// One option's row: its label on the left, its note under the label when it has one, and its control on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    GlobalShortcut,
    Theme,
    DefaultTolerance,
    AlsoShowLogicalPixels,
    ColorFormat,
}

impl Row {
    pub fn label(self) -> &'static str {
        match self {
            Row::GlobalShortcut => "Global shortcut",
            Row::Theme => "Theme",
            Row::DefaultTolerance => "Default tolerance",
            Row::AlsoShowLogicalPixels => "Also show logical pixels",
            Row::ColorFormat => "Color format",
        }
    }

    pub fn note(self) -> Option<&'static str> {
        match self {
            Row::DefaultTolerance => Some("The mouse wheel adjusts it while measuring."),
            Row::AlsoShowLogicalPixels => Some("When the display scale isn't 100%."),
            Row::GlobalShortcut | Row::Theme | Row::ColorFormat => None,
        }
    }

    /// The block it is in, and its place among the block's rows, counted from 0.
    fn slot(self) -> (Block, u32) {
        match self {
            Row::GlobalShortcut => (Block::General, 0),
            // The last of General's three: "Start with Windows"' row comes between with its own ticket (WPC-326), so it
            // never moves.
            Row::Theme => (Block::General, 2),
            Row::DefaultTolerance => (Block::Measurements, 0),
            Row::AlsoShowLogicalPixels => (Block::Measurements, 1),
            Row::ColorFormat => (Block::Inspect, 0),
        }
    }
}

/// The rows ScreenGauge draws, top to bottom, except the Snip's, which `snip_card` places. General's "Start with
/// Windows" comes with its own ticket (WPC-326), into the room its card already has.
pub const ROWS: [Row; 5] = [
    Row::GlobalShortcut,
    Row::Theme,
    Row::DefaultTolerance,
    Row::AlsoShowLogicalPixels,
    Row::ColorFormat,
];

/// The card's border, and the room left and right of a row's label and control inside it (`.srow`'s padding).
pub const CARD_BORDER: u32 = 1;
pub const ROW_PADDING: u32 = 12;

/// A row's label (`#settings`' `font: 13px/1.3`) and note (`.note`: 11.5 tall on the same line-height, 1 below the
/// label): their sizes, their lines, and the room between them.
pub const LABEL_TEXT: f32 = 13.0;
pub const LABEL_LINE: f32 = 13.0 * 1.3;
pub const NOTE_TEXT: f32 = 11.5;
pub const NOTE_LINE: f32 = 11.5 * 1.3;
pub const NOTE_GAP: f32 = 1.0;

impl Block {
    /// Its place in `BLOCKS`, and so on the page.
    fn index(self) -> usize {
        match self {
            Block::General => 0,
            Block::Measurements => 1,
            Block::Inspect => 2,
            Block::Snip => 3,
        }
    }

    /// How many rows the spec gives the block's card: they share its height, a 1-pixel line between two.
    fn rows(self) -> u32 {
        match self {
            Block::General => 3,
            Block::Measurements => 2,
            Block::Inspect => 1,
            // None of the Snip's parts is a `Row`: its card has its own shape (the folder's line and its note), which
            // `snip_card` lays out.
            Block::Snip => 0,
        }
    }
}

/// A row grown by `by` logical pixels to hold a message under its label: its card, everything below it and the page grow
/// as much.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grown {
    pub row: Row,
    pub by: u32,
}

impl Grown {
    /// How much it adds to `block`'s card.
    fn in_block(grown: Option<Grown>, block: Block) -> u32 {
        grown.map_or(0, |grown| {
            if grown.row.slot().0 == block {
                grown.by
            } else {
                0
            }
        })
    }
}

/// A row in its card: from below the line above it, or the card's border, to the line or the border below it, and
/// whether a line runs above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedRow {
    pub row: Row,
    pub span: Span,
    pub line_above: bool,
}

/// Where `row` is on the page, with `grown` taller should it be shown. A card's rows share the room inside its border,
/// the first ones a pixel taller when it doesn't divide evenly; the grown row has its share and the growth.
pub fn place_row(row: Row, grown: Option<Grown>) -> PlacedRow {
    let (block, index) = row.slot();
    let placed = page(grown).blocks[block.index()];
    let count = block.rows();
    let inside = block.card() - 2 * CARD_BORDER;
    let room = inside - (count - 1);
    let (each, extra) = (room / count, room % count);
    let growth = |i: u32| match grown {
        Some(grown) if grown.row.slot() == (block, i) => grown.by,
        _ => 0,
    };
    let height = |i: u32| each + u32::from(i < extra) + growth(i);
    let top = placed.card.top + CARD_BORDER + (0..index).map(|i| height(i) + 1).sum::<u32>();
    PlacedRow {
        row,
        span: Span {
            top,
            bottom: top + height(index),
        },
        line_above: index > 0,
    }
}

/// The page, laid out, with `grown` taller should it be shown.
pub fn page(grown: Option<Grown>) -> Page {
    let mut top = PADDING;
    let blocks = BLOCKS.map(|block| {
        let heading = top;
        let card = Span {
            top: heading + HEADING,
            bottom: heading + HEADING + block.card() + Grown::in_block(grown, block),
        };
        top = card.bottom + GAP;
        Placed {
            block,
            heading,
            card,
        }
    });
    // The last block is followed by the padding, not by a gap.
    let footer_top = top - GAP + PADDING;
    let height = footer_top + FOOTER;
    Page {
        blocks,
        footer: Span {
            top: footer_top,
            bottom: height,
        },
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_as_tall_as_the_prototype_without_the_language_row_in_whole_pixels() {
        // Measured in a browser for WPC-319: 588.56 for the prototype's window, less its 32 of drawn title bar.
        let measured = 556.56;
        let height = f64::from(page(None).height);
        assert!(
            (measured..measured + 2.0).contains(&height),
            "{height} for the {measured} measured"
        );
    }

    #[test]
    fn the_blocks_come_in_the_spec_s_order_one_below_the_other_with_the_footer_last() {
        let page = page(None);
        let order: Vec<Block> = page.blocks.iter().map(|placed| placed.block).collect();
        assert_eq!(
            order,
            [
                Block::General,
                Block::Measurements,
                Block::Inspect,
                Block::Snip
            ]
        );
        assert_eq!(page.blocks[0].heading, 12);
        for pair in page.blocks.windows(2) {
            assert_eq!(pair[0].card.bottom + 10, pair[1].heading, "{pair:?}");
        }
        for placed in &page.blocks {
            assert_eq!(placed.card.top, placed.heading + 16, "{placed:?}");
        }
        // The footer starts 12 below the last card and ends the page.
        assert_eq!(page.footer.top, page.blocks[3].card.bottom + 12);
        assert_eq!(page.footer.bottom - page.footer.top, 36);
        assert_eq!(page.footer.bottom, page.height);
    }

    #[test]
    fn each_card_is_as_tall_as_the_prototype_s() {
        let measured = [128.0, 87.69, 43.33, 145.28];
        for (placed, measured) in page(None).blocks.iter().zip(measured) {
            let height = f64::from(placed.card.bottom - placed.card.top);
            assert!(
                (height - measured).abs() <= 0.5,
                "{:?}: {height} for the {measured} measured",
                placed.block
            );
        }
    }

    #[test]
    fn every_block_s_index_is_its_place_in_the_list() {
        for (index, block) in BLOCKS.iter().enumerate() {
            assert_eq!(block.index(), index, "{block:?}");
        }
    }

    #[test]
    fn measurements_two_rows_share_its_card_with_a_line_between_them() {
        let card = page(None).blocks[1].card;
        let tolerance = place_row(Row::DefaultTolerance, None);
        let logical = place_row(Row::AlsoShowLogicalPixels, None);
        // Inside the card's border, one below the other, a pixel apart for the line, filling the card.
        assert_eq!(tolerance.span.top, card.top + 1);
        assert_eq!(logical.span.top, tolerance.span.bottom + 1);
        assert_eq!(logical.span.bottom, card.bottom - 1);
        assert!(!tolerance.line_above && logical.line_above);
        // The prototype's rows with a note are 42.84 and 43.51 tall, its line included in the second.
        let heights = [
            tolerance.span.bottom - tolerance.span.top,
            logical.span.bottom - logical.span.top,
        ];
        assert_eq!(heights, [43, 42]);
    }

    #[test]
    fn theme_is_the_last_of_general_s_three_rows_against_the_card_s_bottom() {
        let card = page(None).blocks[0].card;
        let theme = place_row(Row::Theme, None);
        // General's 126 pixels inside its border: three rows of 42, 41 and 41, and two lines between them.
        assert_eq!(theme.span.bottom, card.bottom - 1);
        assert_eq!(theme.span.top, card.top + 1 + 42 + 1 + 41 + 1);
        assert!(theme.line_above);
    }

    #[test]
    fn inspect_s_one_row_fills_its_card() {
        let card = page(None).blocks[2].card;
        let color = place_row(Row::ColorFormat, None);
        assert_eq!(
            color.span,
            Span {
                top: card.top + 1,
                bottom: card.bottom - 1
            }
        );
        assert!(!color.line_above);
    }

    #[test]
    fn the_rows_have_the_spec_s_labels_and_notes() {
        let texts: Vec<(&str, Option<&str>)> =
            ROWS.iter().map(|row| (row.label(), row.note())).collect();
        assert_eq!(
            texts,
            [
                ("Global shortcut", None),
                ("Theme", None),
                (
                    "Default tolerance",
                    Some("The mouse wheel adjusts it while measuring.")
                ),
                (
                    "Also show logical pixels",
                    Some("When the display scale isn't 100%.")
                ),
                ("Color format", None),
            ]
        );
    }

    #[test]
    fn the_global_shortcut_is_general_s_first_row_against_the_card_s_top() {
        let card = page(None).blocks[0].card;
        let shortcut = place_row(Row::GlobalShortcut, None);
        assert_eq!(
            shortcut.span,
            Span {
                top: card.top + 1,
                bottom: card.top + 1 + 42
            }
        );
        assert!(!shortcut.line_above);
    }

    #[test]
    fn a_message_makes_its_row_its_card_and_the_page_taller_and_moves_what_is_below() {
        let grown = Some(Grown {
            row: Row::GlobalShortcut,
            by: 31,
        });
        let (before, after) = (page(None), page(grown));
        assert_eq!(after.height, before.height + 31);
        assert_eq!(after.blocks[0].card.top, before.blocks[0].card.top);
        assert_eq!(
            after.blocks[0].card.bottom,
            before.blocks[0].card.bottom + 31
        );
        for (was, is) in before.blocks.iter().zip(&after.blocks).skip(1) {
            assert_eq!(is.heading, was.heading + 31, "{:?}", is.block);
            assert_eq!(is.card.top, was.card.top + 31, "{:?}", is.block);
            assert_eq!(is.card.bottom, was.card.bottom + 31, "{:?}", is.block);
        }
        assert_eq!(after.footer.top, before.footer.top + 31);
        assert_eq!(after.footer.bottom, after.height);

        // The row itself is 31 taller from where it was; the rows below it in its card move down as much.
        let shortcut = place_row(Row::GlobalShortcut, grown);
        assert_eq!(
            shortcut.span.top,
            place_row(Row::GlobalShortcut, None).span.top
        );
        assert_eq!(shortcut.span.bottom - shortcut.span.top, 42 + 31);
        let theme = place_row(Row::Theme, grown);
        assert_eq!(theme.span.top, place_row(Row::Theme, None).span.top + 31);
        assert_eq!(theme.span.bottom, after.blocks[0].card.bottom - 1);
        for row in [
            Row::DefaultTolerance,
            Row::AlsoShowLogicalPixels,
            Row::ColorFormat,
        ] {
            let (was, is) = (place_row(row, None), place_row(row, grown));
            assert_eq!(is.span.top, was.span.top + 31, "{row:?}");
            assert_eq!(is.span.bottom, was.span.bottom + 31, "{row:?}");
        }
    }

    #[test]
    fn with_no_message_the_page_is_as_it_always_was() {
        assert_eq!(page(None).height, 558);
        let grown_by_nothing = Some(Grown {
            row: Row::GlobalShortcut,
            by: 0,
        });
        assert_eq!(page(grown_by_nothing), page(None));
    }

    #[test]
    fn the_headings_are_the_blocks_names_in_capitals() {
        let headings: Vec<&str> = BLOCKS.iter().map(|block| block.heading()).collect();
        assert_eq!(headings, ["GENERAL", "MEASUREMENTS", "INSPECT", "SNIP"]);
    }
}
