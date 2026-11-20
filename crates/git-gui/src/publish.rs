//! Creating a repository and publishing one: the name rules, the built-in
//! `.gitignore` and license templates (GitHub's own list is used when
//! signed in), and what publishing does with the remote.

/// A repository name GitHub accepts: letters, digits, `-`, `_`, and `.`,
/// with anything else turned into `-` as GitHub does.
pub fn sanitize_repository_name(typed: &str) -> String {
    let mut out = String::new();
    for c in typed.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// Templates that work offline.
pub const GITIGNORES: &[(&str, &str)] = &[
    ("Rust", "/target\n**/*.rs.bk\n*.pdb\n"),
    (
        "Node",
        "node_modules/\nnpm-debug.log*\nyarn-error.log*\ndist/\n.env\n",
    ),
    (
        "Python",
        "__pycache__/\n*.py[cod]\n.venv/\nvenv/\nbuild/\ndist/\n*.egg-info/\n",
    ),
    ("Go", "*.exe\n*.test\n*.out\nvendor/\n"),
    ("Java", "*.class\n*.jar\ntarget/\nbuild/\n.gradle/\n"),
    ("C", "*.o\n*.a\n*.so\n*.out\nbuild/\n"),
];

pub const LICENSES: &[(&str, &str)] = &[("MIT", MIT), ("ISC", ISC), ("Unlicense", UNLICENSE)];

const MIT: &str = "MIT License\n\nCopyright (c) [year] [fullname]\n\nPermission is hereby granted, free of charge, to any person obtaining a copy\nof this software and associated documentation files (the \"Software\"), to deal\nin the Software without restriction, including without limitation the rights\nto use, copy, modify, merge, publish, distribute, sublicense, and/or sell\ncopies of the Software, and to permit persons to whom the Software is\nfurnished to do so, subject to the following conditions:\n\nThe above copyright notice and this permission notice shall be included in all\ncopies or substantial portions of the Software.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR\nIMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,\nFITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE\nAUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER\nLIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,\nOUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE\nSOFTWARE.\n";

const ISC: &str = "ISC License\n\nCopyright (c) [year] [fullname]\n\nPermission to use, copy, modify, and/or distribute this software for any\npurpose with or without fee is hereby granted, provided that the above\ncopyright notice and this permission notice appear in all copies.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\" AND THE AUTHOR DISCLAIMS ALL WARRANTIES\nWITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF\nMERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR\nANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES\nWHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN\nACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF\nOR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.\n";

const UNLICENSE: &str = "This is free and unencumbered software released into the public domain.\n\nAnyone is free to copy, modify, publish, use, compile, sell, or\ndistribute this software, either in source code form or as a compiled\nbinary, for any purpose, commercial or non-commercial, and by any\nmeans.\n\nFor more information, please refer to <https://unlicense.org>\n";

/// A license text with the year and name filled in.
pub fn fill_license(text: &str, year: i32, name: &str) -> String {
    text.replace("[year]", &year.to_string())
        .replace("[fullname]", name)
}

/// The year on the local wall clock at `now`, for a licence's copyright
/// line: on 1 January it is the new year from local midnight, not from
/// midnight UTC (ticket 146).
pub fn current_year(now: i64) -> i32 {
    current_year_in(now, better_core::local_time::TimeZone::System)
}

/// [`current_year`] in a given zone.
pub fn current_year_in(now: i64, zone: better_core::local_time::TimeZone) -> i32 {
    i32::try_from(zone.at(now).civil.year).unwrap_or(i32::MAX)
}

/// A README for a new repository.
pub fn readme(name: &str, description: &str) -> String {
    let mut text = format!("# {name}\n");
    if !description.trim().is_empty() {
        text.push('\n');
        text.push_str(description.trim());
        text.push('\n');
    }
    text
}
