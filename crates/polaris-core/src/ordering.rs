//! Ordering of the text lists shown on the dashboard (table rows, alerts,
//! chart series), so every renderer displays them alphabetically.

use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

const ZERO_DIGIT: char = '0';

/// Compares two texts alphabetically, ignoring case and comparing runs of
/// digits by their numeric value: `apu < Barney < homer2 < homer10 < Moe`.
///
/// Texts equal under those rules fall back to their exact characters so
/// the order stays total and deterministic.
pub fn natural_order(left_text: &str, right_text: &str) -> Ordering {
    compare_chunks(
        &mut left_text.chars().peekable(),
        &mut right_text.chars().peekable(),
    )
    .then_with(|| left_text.cmp(right_text))
}

/// Sorts `items` by the text `text_of` returns, using [`natural_order`].
/// The sort is stable: items with the same text keep their order.
pub fn sort_naturally<T>(items: &mut [T], text_of: impl Fn(&T) -> &str) {
    items.sort_by(|left_item, right_item| natural_order(text_of(left_item), text_of(right_item)));
}

fn compare_chunks(
    left_characters: &mut Peekable<Chars>,
    right_characters: &mut Peekable<Chars>,
) -> Ordering {
    loop {
        let ordering = match (left_characters.peek(), right_characters.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(left_character), Some(right_character))
                if left_character.is_ascii_digit() && right_character.is_ascii_digit() =>
            {
                compare_numbers(
                    &take_digits(left_characters),
                    &take_digits(right_characters),
                )
            }
            (Some(_), Some(_)) => compare_ignoring_case(left_characters, right_characters),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
}

fn compare_ignoring_case(
    left_characters: &mut Peekable<Chars>,
    right_characters: &mut Peekable<Chars>,
) -> Ordering {
    match (left_characters.next(), right_characters.next()) {
        (Some(left_character), Some(right_character)) => left_character
            .to_lowercase()
            .cmp(right_character.to_lowercase()),
        _ => Ordering::Equal,
    }
}

fn take_digits(characters: &mut Peekable<Chars>) -> String {
    let mut digits = String::new();
    while let Some(digit) = characters.next_if(char::is_ascii_digit) {
        digits.push(digit);
    }
    digits
}

/// Compares digit runs by value without parsing, so any length works.
fn compare_numbers(left_digits: &str, right_digits: &str) -> Ordering {
    let left_significant = left_digits.trim_start_matches(ZERO_DIGIT);
    let right_significant = right_digits.trim_start_matches(ZERO_DIGIT);
    left_significant
        .len()
        .cmp(&right_significant.len())
        .then_with(|| left_significant.cmp(right_significant))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_ignoring_case_and_by_number_value() {
        let mut names = vec!["Moe", "homer10", "apu", "homer2", "Barney"];

        sort_naturally(&mut names, |name| name);

        assert_eq!(names, ["apu", "Barney", "homer2", "homer10", "Moe"]);
    }

    #[test]
    fn case_variants_sort_together() {
        let mut nodes = vec!["talos-node10", "Talos-CP0", "talos-node2", "Moe"];

        sort_naturally(&mut nodes, |node| node);

        assert_eq!(nodes, ["Moe", "Talos-CP0", "talos-node2", "talos-node10"]);
    }

    #[test]
    fn ties_fall_back_to_exact_text() {
        assert_eq!(natural_order("homer", "Homer"), "homer".cmp("Homer"));
        assert_eq!(natural_order("homer01", "homer1"), "homer01".cmp("homer1"));
        assert_eq!(natural_order("Barney", "Barney"), Ordering::Equal);
    }

    #[test]
    fn longer_numbers_are_larger() {
        assert_eq!(
            natural_order("homer99999999999999999999", "homer100000000000000000000"),
            Ordering::Less
        );
    }
}
