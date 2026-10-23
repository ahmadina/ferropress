//! Row-level differences between two database dumps.
//!
//! Input is `mysqldump --skip-extended-insert --compact` output: one ``INSERT INTO `table` VALUES (...);``
//! line per row. A changed row appears as one removed and one added row.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::Serialize;

/// Rows that differ in one table, each list sorted.
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct TableDiff {
    pub(crate) added: Vec<String>,
    pub(crate) removed: Vec<String>,
}

/// Changed tables by name; unchanged tables are omitted.
pub(crate) type DatabaseDiff = BTreeMap<String, TableDiff>;

fn rows_by_table(dump: &str) -> BTreeMap<&str, Vec<&str>> {
    let mut tables: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for line in dump.lines() {
        let Some(rest) = line.strip_prefix("INSERT INTO `") else {
            continue;
        };
        let Some((table, values)) = rest.split_once("` VALUES ") else {
            continue;
        };
        let row = values.strip_suffix(';').unwrap_or(values);
        tables.entry(table).or_default().push(row);
    }
    for rows in tables.values_mut() {
        rows.sort_unstable();
    }
    tables
}

/// Multiset difference of two sorted row lists.
fn diff_sorted(before: &[&str], after: &[&str]) -> TableDiff {
    let mut diff = TableDiff::default();
    let (mut old, mut new) = (before.iter().peekable(), after.iter().peekable());
    loop {
        match (old.peek(), new.peek()) {
            (Some(a), Some(b)) => match a.cmp(b) {
                Ordering::Equal => {
                    old.next();
                    new.next();
                }
                Ordering::Less => diff.removed.extend(old.next().map(|row| (*row).to_owned())),
                Ordering::Greater => diff.added.extend(new.next().map(|row| (*row).to_owned())),
            },
            (Some(_), None) => diff
                .removed
                .extend(old.by_ref().map(|row| (*row).to_owned())),
            (None, Some(_)) => diff.added.extend(new.by_ref().map(|row| (*row).to_owned())),
            (None, None) => return diff,
        }
    }
}

/// Rows added and removed between two dumps, per table.
pub(crate) fn diff(before: &str, after: &str) -> DatabaseDiff {
    let old = rows_by_table(before);
    let new = rows_by_table(after);
    let mut tables: Vec<&str> = old.keys().chain(new.keys()).copied().collect();
    tables.sort_unstable();
    tables.dedup();
    tables
        .into_iter()
        .filter_map(|table| {
            let rows = diff_sorted(
                old.get(table).map_or(&[][..], Vec::as_slice),
                new.get(table).map_or(&[][..], Vec::as_slice),
            );
            let changed = !rows.added.is_empty() || !rows.removed.is_empty();
            changed.then(|| (table.to_owned(), rows))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEFORE: &str = "\
/*!40101 SET @saved_cs_client = @@character_set_client */;
INSERT INTO `options` VALUES (1,'siteurl','http://a');
INSERT INTO `options` VALUES (2,'blogname','Old');
INSERT INTO `posts` VALUES (1,'Hello');
INSERT INTO `gone` VALUES (1);
";

    const AFTER: &str = "\
INSERT INTO `options` VALUES (2,'blogname','New');
INSERT INTO `options` VALUES (1,'siteurl','http://a');
INSERT INTO `posts` VALUES (1,'Hello');
INSERT INTO `fresh` VALUES (7);
";

    fn strings(rows: &[&str]) -> Vec<String> {
        rows.iter().map(|&row| row.to_owned()).collect()
    }

    #[test]
    fn identical_dumps_have_no_diff() {
        assert_eq!(diff(BEFORE, BEFORE), DatabaseDiff::new());
    }

    #[test]
    fn row_order_does_not_matter() {
        let reordered = "INSERT INTO `t` VALUES (2);\nINSERT INTO `t` VALUES (1);\n";
        let sorted = "INSERT INTO `t` VALUES (1);\nINSERT INTO `t` VALUES (2);\n";
        assert_eq!(diff(reordered, sorted), DatabaseDiff::new());
    }

    #[test]
    fn changes_additions_and_removals_are_reported_per_table() {
        let diff = diff(BEFORE, AFTER);
        assert_eq!(
            diff.keys().map(String::as_str).collect::<Vec<_>>(),
            ["fresh", "gone", "options"]
        );
        assert_eq!(
            diff["options"],
            TableDiff {
                added: strings(&["(2,'blogname','New')"]),
                removed: strings(&["(2,'blogname','Old')"]),
            }
        );
        assert_eq!(diff["fresh"].added, strings(&["(7)"]));
        assert!(diff["fresh"].removed.is_empty());
        assert_eq!(diff["gone"].removed, strings(&["(1)"]));
        assert!(diff["gone"].added.is_empty());
    }

    #[test]
    fn duplicate_rows_are_counted() {
        let one = "INSERT INTO `log` VALUES ('x');\n";
        let two = "INSERT INTO `log` VALUES ('x');\nINSERT INTO `log` VALUES ('x');\n";
        assert_eq!(diff(one, two)["log"].added, strings(&["('x')"]));
        assert_eq!(diff(two, one)["log"].removed, strings(&["('x')"]));
    }

    #[test]
    fn non_insert_and_malformed_lines_are_ignored() {
        let noise = "-- comment\nINSERT INTO `broken\nINSERT INTO `t` VALUES (1)\n";
        let diff = diff("", noise);
        assert_eq!(diff["t"].added, strings(&["(1)"]));
        assert_eq!(diff.len(), 1);
    }

    #[test]
    fn values_containing_the_separator_stay_intact() {
        let row = "INSERT INTO `t` VALUES ('a` VALUES b');\n";
        assert_eq!(diff("", row)["t"].added, strings(&["('a` VALUES b')"]));
    }

    #[test]
    fn diff_serializes_as_tables_of_rows() {
        let json = serde_json::to_string(&diff("", "INSERT INTO `t` VALUES (1);")).unwrap();
        assert_eq!(json, r#"{"t":{"added":["(1)"],"removed":[]}}"#);
    }
}
