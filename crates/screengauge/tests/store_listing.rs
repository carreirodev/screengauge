//! What ScreenGauge's Microsoft Store page says (WPC-341): `packaging/store/listing.md` holds every text pasted into
//! Partner Center, one `## ` section per field. These tests hold it to the limits Microsoft Learn gives for each field
//! (packaging/store/README.md has the sources), to the name as the spec writes it, ScreenGauge, and to the spec's
//! rules for the page: English only, with none of WPC-286's goals, and the privacy policy the README links to.

const LISTING: &str = include_str!("../../../packaging/store/listing.md");
const README: &str = include_str!("../../../README.md");
const README_PT_BR: &str = include_str!("../../../README.pt-BR.md");

/// The fields, in the order the file gives them.
const FIELDS: [&str; 8] = [
    "Product name",
    "Short description",
    "Description",
    "Product features",
    "Screenshots",
    "Privacy policy URL",
    "Website",
    "Restricted capability: runFullTrust",
];

/// The fields the Store page shows, and the justification the certification testers read: everything pasted as text.
const TEXT_FIELDS: [&str; 5] = [
    "Short description",
    "Description",
    "Product features",
    "Screenshots",
    "Restricted capability: runFullTrust",
];

/// Every `## ` section of the listing, as its heading and its body without the blank lines around it.
fn sections() -> Vec<(&'static str, String)> {
    let mut found: Vec<(&str, Vec<&str>)> = Vec::new();
    for line in LISTING.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            found.push((heading, Vec::new()));
        } else if let Some((_, body)) = found.last_mut() {
            body.push(line);
        }
    }
    found
        .into_iter()
        .map(|(heading, body)| (heading, body.join("\n").trim().to_string()))
        .collect()
}

fn field(name: &str) -> String {
    sections()
        .into_iter()
        .find(|(heading, _)| *heading == name)
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("listing.md has no \"## {name}\" section"))
}

/// The items of a list field: "- item" for the features, "1. item" for the screenshots.
fn items(name: &str) -> Vec<String> {
    field(name)
        .lines()
        .map(|line| {
            let item = line
                .strip_prefix("- ")
                .or_else(|| line.split_once(". ").map(|(_, item)| item))
                .unwrap_or_else(|| panic!("\"{line}\" in {name} is not a list item"));
            item.trim().to_string()
        })
        .collect()
}

fn chars(text: &str) -> usize {
    text.chars().count()
}

#[test]
fn the_listing_has_each_field_once_in_order() {
    let headings: Vec<&str> = sections().into_iter().map(|(heading, _)| heading).collect();
    assert_eq!(headings, FIELDS);
    for (heading, body) in sections() {
        assert!(!body.is_empty(), "{heading} is empty");
    }
}

#[test]
fn the_product_name_is_the_reserved_one() {
    assert_eq!(field("Product name"), "ScreenGauge");
}

#[test]
fn each_field_fits_microsoft_s_limits() {
    // Description: required, "up to 10,000 characters of plain text".
    assert!(chars(&field("Description")) <= 10_000);
    // Short description: the field takes 1000, "but in some views, only the first 270 characters will be shown".
    let short = field("Short description");
    assert!(chars(&short) <= 270, "{} characters", chars(&short));
    // Product features: "no more than 200 characters per feature. You may include up to 20 features."
    let features = items("Product features");
    assert!(
        (1..=20).contains(&features.len()),
        "{} features",
        features.len()
    );
    for feature in features {
        assert!(chars(&feature) <= 200, "{feature}");
    }
    // Screenshots: "up to 10 desktop screenshots", "at least four" recommended, captions "200 characters or less".
    let screenshots = items("Screenshots");
    assert!(
        (4..=10).contains(&screenshots.len()),
        "{} screenshots",
        screenshots.len()
    );
    for caption in screenshots {
        assert!(chars(&caption) <= 200, "{caption}");
    }
}

#[test]
fn the_screenshots_are_numbered_in_order() {
    for (index, line) in field("Screenshots").lines().enumerate() {
        let number = format!("{}. ", index + 1);
        assert!(
            line.starts_with(&number),
            "\"{line}\" should start with \"{number}\""
        );
    }
}

#[test]
fn the_texts_are_plain_english() {
    for name in TEXT_FIELDS {
        let text = field(name);
        // English with no Portuguese accents, and plain text: Partner Center shows markup and links as typed, and
        // Microsoft asks for "no HTML, code snippets, or URLs in the description field".
        if let Some(c) = text.chars().find(|c| !c.is_ascii()) {
            panic!("{name} has '{c}', not plain ASCII text");
        }
        for markup in ["*", "`", "[", "]", "<", ">", "#", "http", "www."] {
            assert!(!text.contains(markup), "{name} has \"{markup}\"");
        }
    }
}

#[test]
fn screengauge_is_one_word_with_s_and_g_capitalized() {
    for name in TEXT_FIELDS {
        let text = field(name);
        let lower = text.to_ascii_lowercase();
        assert!(!lower.contains("pixmeter"), "{name} has the old name");
        let mut from = 0;
        while let Some(at) = lower[from..].find("screen").map(|at| from + at) {
            let rest = lower[at + "screen".len()..].trim_start_matches([' ', '-', '_']);
            if rest.starts_with("gauge") {
                let written = &text[at..lower.len() - rest.len() + "gauge".len()];
                assert_eq!(written, "ScreenGauge", "in {name}");
            }
            from = at + "screen".len();
        }
    }
}

/// Whether `text` has a figure of the kind WPC-286's goals give: a number followed by a unit of time, size or share.
fn goal_figure(text: &str) -> Option<String> {
    const UNITS: [&str; 12] = [
        "ms",
        "s",
        "sec",
        "secs",
        "second",
        "seconds",
        "milliseconds",
        "kb",
        "mb",
        "gb",
        "megabytes",
        "%",
    ];
    let words: Vec<String> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')' | '.'))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    for (index, word) in words.iter().enumerate() {
        let after_digits = word.trim_start_matches(|c: char| c.is_ascii_digit());
        if after_digits.len() == word.len() {
            continue;
        }
        // "250ms", "5mb", "100%" in one word, or "250 ms" in two.
        let found = if after_digits.is_empty() {
            words
                .get(index + 1)
                .filter(|unit| UNITS.contains(&unit.as_str()))
                .map(|unit| format!("{word} {unit}"))
        } else {
            UNITS.contains(&after_digits).then(|| word.clone())
        };
        if found.is_some() {
            return found;
        }
    }
    ["cpu", "processor", "ram", "megabyte"]
        .into_iter()
        .find(|named| words.iter().any(|word| word.starts_with(named)))
        .map(String::from)
}

#[test]
fn a_goal_figure_is_caught() {
    // A test of the check itself, with figures as WPC-286 writes them.
    for figure in [
        "opens in 250 ms",
        "under 5MB",
        "uses 0% CPU when idle",
        "40 MB of memory",
        "in 1.5 seconds",
    ] {
        assert!(goal_figure(figure).is_some(), "{figure}");
    }
    for fine in [
        "Windows 11",
        "the keys 1 to 4",
        "10 pixels with Shift",
        "Win + Shift + X",
    ] {
        assert_eq!(goal_figure(fine), None, "{fine}");
    }
}

#[test]
fn none_of_wpc_286_s_goals_show() {
    for name in TEXT_FIELDS {
        if let Some(figure) = goal_figure(&field(name)) {
            panic!("{name} gives a goal figure: {figure}");
        }
    }
}

#[test]
fn the_privacy_policy_is_the_one_the_readme_links_to() {
    let url = field("Privacy policy URL");
    assert!(url.starts_with("https://"), "{url}");
    assert!(!url.contains(char::is_whitespace), "{url}");
    assert!(
        README.contains(&format!("]({url})")),
        "README.md doesn't link to {url}"
    );
    assert!(
        README_PT_BR.contains(&format!("]({url})")),
        "README.pt-BR.md doesn't link to {url}"
    );
}

#[test]
fn the_website_is_the_project_page() {
    // The page Settings' "Project page" opens.
    assert_eq!(field("Website"), env!("CARGO_PKG_REPOSITORY"));
}
