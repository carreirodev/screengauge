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
//! The page measured 556.56 tall; in whole pixels it is 558. The window's height is fixed at that: the user doesn't
//! change it, and the blocks' rows, which the tickets after WPC-319 draw, go inside these cards.

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

/// The page, laid out.
pub fn page() -> Page {
    let mut top = PADDING;
    let blocks = BLOCKS.map(|block| {
        let heading = top;
        let card = Span {
            top: heading + HEADING,
            bottom: heading + HEADING + block.card(),
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
        let height = f64::from(page().height);
        assert!(
            (measured..measured + 2.0).contains(&height),
            "{height} for the {measured} measured"
        );
    }

    #[test]
    fn the_blocks_come_in_the_spec_s_order_one_below_the_other_with_the_footer_last() {
        let page = page();
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
        for (placed, measured) in page().blocks.iter().zip(measured) {
            let height = f64::from(placed.card.bottom - placed.card.top);
            assert!(
                (height - measured).abs() <= 0.5,
                "{:?}: {height} for the {measured} measured",
                placed.block
            );
        }
    }

    #[test]
    fn the_headings_are_the_blocks_names_in_capitals() {
        let headings: Vec<&str> = BLOCKS.iter().map(|block| block.heading()).collect();
        assert_eq!(headings, ["GENERAL", "MEASUREMENTS", "INSPECT", "SNIP"]);
    }
}
