pub(super) struct Edit {
    pub path: &'static str,
    pub old: &'static str,
    pub new: &'static str,
}
pub(super) struct Task {
    pub id: &'static str,
    pub prompt: &'static str,
    pub files: &'static [(&'static str, &'static str)],
    pub edits: &'static [Edit],
    pub tests: &'static str,
    pub conflict: bool,
    pub wrong: Option<&'static str>,
}

pub(super) const TASKS: &[Task] = &[
    Task {
        id: "arithmetic",
        prompt: "Fix combine(a, b) so it returns the sum for signed integers. Preserve the public API. Inspect source, make the smallest fix and run_tests before finishing.",
        files: &[(
            "src/lib.rs",
            "pub fn combine(a: i32, b: i32) -> i32 { a - b }\n",
        )],
        edits: &[Edit {
            path: "src/lib.rs",
            old: "a - b",
            new: "a + b",
        }],
        tests: "#[test] fn sums() { for a in -20..20 { for b in -20..20 { assert_eq!(solution::combine(a,b), a+b); } } }",
        conflict: false,
        wrong: Some("a * b"),
    },
    Task {
        id: "unicode",
        prompt: "Fix count(text) to count Unicode scalar values, not UTF-8 bytes or grapheme clusters. Inspect source, preserve the signature and run_tests.",
        files: &[(
            "src/lib.rs",
            "pub fn count(text: &str) -> usize { text.len() }\n",
        )],
        edits: &[Edit {
            path: "src/lib.rs",
            old: "text.len()",
            new: "text.chars().count()",
        }],
        tests: "#[test] fn scalars() { for text in [\"\", \"hello\", \"नमस्ते\", \"🦀\", \"á\", \"👨‍👩‍👧\", \"你好\"] { assert_eq!(solution::count(text), text.chars().count()); } }",
        conflict: false,
        wrong: None,
    },
    Task {
        id: "boundary-recovery",
        prompt: "Fix in_range(x, low, high) to include both boundaries. Reversed bounds must return false. Inspect source, preserve the signature, recover from edit conflicts by re-reading, and run_tests.",
        files: &[(
            "src/lib.rs",
            "pub fn in_range(x: i32, low: i32, high: i32) -> bool { x > low && x < high }\n",
        )],
        edits: &[Edit {
            path: "src/lib.rs",
            old: "x > low && x < high",
            new: "x >= low && x <= high",
        }],
        tests: "#[test] fn inclusive() { for low in -4..4 { for high in -4..4 { for x in -5..5 { assert_eq!(solution::in_range(x,low,high), x >= low && x <= high); } } } }",
        conflict: true,
        wrong: None,
    },
    Task {
        id: "multi-file",
        prompt: "Fix bill(total, percent) to return the remaining amount after a percentage discount. Inputs are nonnegative, percent is 0..100, and the discount is rounded down. There are two related bugs across src/lib.rs and src/math.rs. Inspect both files, fix both, and run_tests.",
        files: &[
            (
                "src/lib.rs",
                "mod math;\npub fn bill(total: u32, percent: u32) -> u32 { math::discount(total, percent) }\n",
            ),
            (
                "src/math.rs",
                "pub fn discount(amount: u32, percent: u32) -> u32 { (amount / 100) * percent }\n",
            ),
        ],
        edits: &[
            Edit {
                path: "src/lib.rs",
                old: "{ math::discount(total, percent) }",
                new: "{ total - math::discount(total, percent) }",
            },
            Edit {
                path: "src/math.rs",
                old: "(amount / 100) * percent",
                new: "amount * percent / 100",
            },
        ],
        tests: "#[test] fn discounts() { for total in [0,1,99,100,101,9999] { for percent in [0,1,15,100] { assert_eq!(solution::bill(total,percent), total-total*percent/100); } } }",
        conflict: false,
        wrong: None,
    },
];
