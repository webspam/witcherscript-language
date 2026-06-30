pub(crate) fn suffixed_unique(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    let mut suffix = 1usize;
    loop {
        let candidate = format!("{base}{suffix}");
        if !taken(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

pub(crate) fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

// A digit-led result is not a valid identifier; fall back to camelling the full name.
pub(crate) fn receiver_name(type_name: &str) -> String {
    let name = strict_camel(strip_class_prefix(type_name));
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        return strict_camel(type_name);
    }
    name
}

// Engine class names carry a `C` prefix (CR4Player); a `C` starting a plain word (Cat) stays.
fn strip_class_prefix(name: &str) -> &str {
    let Some(rest) = name.strip_prefix('C') else {
        return name;
    };
    match rest.chars().next() {
        Some(c) if c.is_ascii_uppercase() || c.is_ascii_digit() => rest,
        _ => name,
    }
}

fn strict_camel(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    chars
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if i > 0 && starts_new_word(&chars, i) {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

fn starts_new_word(chars: &[char], i: usize) -> bool {
    if !chars[i].is_ascii_uppercase() {
        return false;
    }
    let previous = chars[i - 1];
    if previous.is_ascii_lowercase() || previous.is_ascii_digit() {
        return true;
    }
    // An acronym's last capital starts the next word when a lowercase follows (SQUIDBanana -> Squid Banana).
    previous.is_ascii_uppercase() && chars.get(i + 1).is_some_and(char::is_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{lowercase_first, receiver_name, suffixed_unique};

    #[rstest]
    #[case::empty("", "")]
    #[case::capitalized("Foo", "foo")]
    #[case::already_lower("foo", "foo")]
    #[case::single_char("X", "x")]
    fn lowercase_first_lowercases_only_initial(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            lowercase_first(input),
            expected,
            "lowercase_first({input:?})"
        );
    }

    #[rstest]
    #[case::free(&[], "x")]
    #[case::first_collision(&["x"], "x1")]
    #[case::runs_until_free(&["x", "x1", "x2"], "x3")]
    fn suffixed_unique_appends_lowest_free_suffix(#[case] taken: &[&str], #[case] expected: &str) {
        let got = suffixed_unique("x", |candidate| taken.contains(&candidate));
        assert_eq!(got, expected, "taken={taken:?}");
    }

    #[rstest]
    #[case::cr4_player("CR4Player", "r4Player")]
    #[case::c_player("CPlayer", "player")]
    #[case::w3_acting_cannon("W3ActingCannon", "w3ActingCannon")]
    #[case::url_separator("URLSeparator", "urlSeparator")]
    #[case::acronym_run("DangerousPotatoSQUIDBanana", "dangerousPotatoSquidBanana")]
    #[case::plain_word("Cat", "cat")]
    #[case::lone_c("C", "c")]
    #[case::digit_after_prefix("C2dArray", "c2dArray")]
    fn receiver_name_derives_camel_case(#[case] type_name: &str, #[case] expected: &str) {
        assert_eq!(
            receiver_name(type_name),
            expected,
            "receiver name for {type_name}"
        );
    }
}
