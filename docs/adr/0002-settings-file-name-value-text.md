# The settings file is `name = value` text, read by ScreenGauge's own code

ScreenGauge keeps its options and its remembered state in `settings.ini`: UTF-8 lines of `name = value`, with notes on lines that start with `#`, read and written by the core (`screengauge_core::settings`) with no library. The file is small and flat (16 values, no lists or nesting), and the spec's two rules for it, that a wrong value goes back to its default on its own and that a damaged file never goes back to the defaults in silence, need code that checks each value by name anyway. A parser would replace only the few lines that split each line at its first `=`. Values are written without quotes or escapes, so a Windows path, backslashes and all, is edited in Notepad as it is. Decided in WPC-298.

## Considered Options

- **TOML with the `toml` crate**: a known format that editors color, but it brings serde and several other crates into the core, which has no dependency today, and every text value and path needs quotes.
- **JSON with `serde_json`**: its quoting and backslash escapes make it the hardest to edit by hand, and it has no notes to list the values each setting takes.
- **The registry, or the Windows `.ini` functions (`GetPrivateProfileString`)**: Windows calls, which the core can't make, since the settings module is tested without Windows; and the registry isn't a file a person can open.

## Consequences

- The installer copy's file is `%LOCALAPPDATA%\ScreenGauge\settings.ini`: local rather than roaming, because the folders and the toolbar's place it remembers belong to one computer. The Store copy's file goes in its package's own folder (WPC-334).
- ScreenGauge reads the file when it starts and rewrites all of it whenever something changes, so notes a person adds are lost. The file's first lines say so and ask to exit ScreenGauge before editing.
- A file is damaged when it isn't UTF-8, when a line is neither blank, a note (`#` or `;`), `name = value`, nor a known name whose `=` was left out, or when it holds no name ScreenGauge knows, as an empty file. Otherwise it is read value by value: a wrong value, a known name without its `=`, or a name written twice goes back to its default; a missing name keeps its default without a word, as in a file from an older version; and a name ScreenGauge doesn't know is dropped. Names and words are read in any letter case.
- A save writes `settings.ini.tmp` and renames it over the file, so a crash or a full disk never leaves half a file. A damaged file is moved to `settings.damaged.ini` (then `settings.damaged-2.ini`, and so on, never replacing an earlier copy) before the defaults take its place. When it can't be moved, it stays as it was, and ScreenGauge still reports the return to the defaults.
