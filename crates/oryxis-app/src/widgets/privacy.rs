//! UI helper widgets: privacy. Split out of widgets/mod.rs.

use iced::Widget as _;
use super::*;
/// Mask a sensitive string for Privacy Mode: every non-whitespace char
/// becomes a muted block (`192.168.0.4` -> `███████████`, `deploy@web`
/// -> `██████████`). Separators are masked too, a visible `.` / `@` / `:`
/// would reveal the value's shape (octet count, username length). Used on
/// host cards and in session logs; the terminal does its own per-cell
/// masking against the same block glyph.
pub(crate) fn mask_blocks(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { c } else { '█' })
        .collect()
}

/// Redact IPv4/IPv6 addresses, `user@host` prompt tokens, home-directory
/// usernames (`C:\Users\<name>`, `/home/<name>`, `/Users/<name>`) and the
/// caller's known-sensitive `terms` (saved-connection hostnames, lowercase,
/// see `Oryxis::privacy_terms`) in arbitrary text for Privacy Mode,
/// replacing each match with muted blocks via [`mask_blocks`]. Used by the
/// session-log viewer (which renders recorded terminal output) so a
/// recording hides the same things the live terminal does. `user@host`
/// also catches emails and typed `ssh user@host` targets, which are
/// sensitive too. For home dirs only the name segment is masked, the
/// surrounding path stays readable. Returns the input unchanged when
/// nothing matches.
pub(crate) fn redact_for_display(
    s: &str,
    terms: &[String],
    classes: oryxis_terminal::PrivacyClasses,
) -> String {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Home dirs and `user@host` first so their tokens win over a bare
        // IP that might sit inside them. Home dirs aren't URL-aware:
        // display masking is reversible via Reveal, so erring toward
        // hiding is acceptable. The IPv4 candidate is shape-only here and
        // validated in code below (octet range), mirroring the terminal
        // widget.
        // The IPv6 candidate is loose (any hex/colon run with 2+ colons,
        // plus an optional dotted-quad tail for `::ffff:192.0.2.1`) and is
        // validated in code below, regex alternation alone can't express
        // "has `::` or exactly 7 colons" without exploding.
        regex::Regex::new(
            r"(?i:[\\/](?:users|home)[\\/])(?P<hd>[A-Za-z0-9._-]+)|[A-Za-z0-9._-]+@[A-Za-z0-9._-]+|(?P<v6>[0-9A-Fa-f]{0,4}(?::[0-9A-Fa-f]{0,4}){2,}(?:\.\d{1,3}\.\d{1,3}\.\d{1,3})?)|(?P<v4>\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b)",
        )
        .expect("privacy display pattern")
    });
    let mut out = if re.is_match(s) {
        let bytes = s.as_bytes();
        let mut out = String::with_capacity(s.len());
        let mut last = 0;
        for caps in re.captures_iter(s) {
            let whole = caps.get(0).expect("regex match always has a group 0");
            if let Some(m) = caps.name("hd") {
                // Home-dir match: mask only the captured name. Gated by
                // the usernames class (issue #78 block 1).
                if classes.usernames {
                    out.push_str(&s[last..m.start()]);
                    out.push_str(&mask_blocks(m.as_str()));
                } else {
                    out.push_str(&s[last..whole.end()]);
                    last = whole.end();
                    continue;
                }
            } else if let Some(m) = caps.name("v6") {
                // IPv6 candidate: trim prose colons, reject runs glued to
                // a word (std::io) or that fail the shared validator.
                let (mut a, mut b) = (m.start(), m.end());
                let core_end = s[a..b].find('.').map(|p| a + p).unwrap_or(b);
                if core_end - a >= 2 && bytes[a] == b':' && bytes[a + 1] != b':' {
                    a += 1;
                }
                let mut ce = core_end;
                if ce > a + 1 && bytes[ce - 1] == b':' && bytes[ce - 2] != b':' {
                    ce -= 1;
                }
                // A trimmed hex core with a dotted tail isn't an embedded
                // IPv4 form anymore; drop the tail from the mask.
                if ce != core_end {
                    b = ce;
                }
                let is_wordish = |x: u8| x.is_ascii_alphanumeric() || x == b'_' || x == b'.';
                let glued = (a > 0 && is_wordish(bytes[a - 1]))
                    || (b < s.len() && (bytes[b].is_ascii_alphanumeric() || bytes[b] == b'_'));
                // Per-class gate (issue #78 block 1), mirroring the
                // terminal widget's split.
                let class_on = if oryxis_terminal::ipv6_is_local(&s[a..ce]) {
                    classes.private_ips
                } else {
                    classes.public_ips
                };
                if !glued && class_on && oryxis_terminal::looks_like_ipv6(&s[a..ce]) {
                    out.push_str(&s[last..a]);
                    out.push_str(&mask_blocks(&s[a..b]));
                    out.push_str(&s[b..whole.end()]);
                } else {
                    out.push_str(&s[last..whole.end()]);
                }
                last = whole.end();
                continue;
            } else if let Some(m) = caps.name("v4") {
                // Quad-dot candidate: adopt the widget's octet-range check
                // (the shape-only regex over-masked `999.1.1.1` before).
                // Every range-valid quad masks per its class: the issue #53
                // version-context exemption was removed 2026-07-19
                // (mirroring the terminal widget), since hostile output
                // could use a "version" prefix to leave a real address
                // readable.
                let text = m.as_str();
                let range_valid = text
                    .split('.')
                    .all(|g| g.parse::<u16>().is_ok_and(|v| v <= 255));
                // Per-class gates (issue #78 block 1): a vault-term hit
                // always masks (the terms list is already
                // class-filtered); otherwise the private / public class
                // switch decides, mirroring the terminal widget.
                let term_hit = terms.iter().any(|t| t == text);
                let private = oryxis_terminal::ipv4_is_private_or_loopback(text);
                let mask = range_valid
                    && (term_hit
                        || (private && classes.private_ips)
                        || (!private && classes.public_ips));
                if mask {
                    out.push_str(&s[last..m.start()]);
                    out.push_str(&mask_blocks(text));
                } else {
                    out.push_str(&s[last..m.end()]);
                }
            } else {
                // The bare `user@host` alternation.
                out.push_str(&s[last..whole.start()]);
                let tok = whole.as_str();
                if classes.usernames {
                    out.push_str(&mask_blocks(tok));
                } else if let Some(at) = tok.find('@') {
                    // Usernames class off: keep the name, but a host part
                    // that is itself an IP must still mask by its own
                    // class. The regex consumes `deploy@8.8.8.8` as one
                    // user@host token, so the v4/v6 scanner never sees the
                    // address; the live terminal masks it independently,
                    // so the log must match. Re-run redaction on just the
                    // host side (a bare hostname there stays, that shape
                    // is the terms list's job, not this token).
                    out.push_str(&tok[..=at]);
                    out.push_str(&redact_for_display(&tok[at + 1..], terms, classes));
                } else {
                    out.push_str(tok);
                }
            }
            last = whole.end();
        }
        out.push_str(&s[last..]);
        out
    } else {
        s.to_string()
    };
    if !terms.is_empty() {
        out = mask_terms(&out, terms);
    }
    out
}

/// Split a user-edited privacy list (the "Always mask" / "Never mask"
/// settings, issue #78): comma / semicolon / newline separated,
/// trimmed, lowercased, empties dropped.
pub(crate) fn parse_privacy_list(s: &str) -> impl Iterator<Item = String> + '_ {
    s.split([',', ';', '\n'])
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
}

/// Assemble the effective Privacy Mode terms (issue #78): the values
/// derived from the vault (saved hostnames + usernames), minus the
/// user's never-mask list, plus the user's always-mask list. The
/// always list is NOT run through the never filter: an explicit add
/// beats the seeded never defaults when the same word sits in both.
/// Everything is lowercased, deduped and held to the same >= 4 chars
/// floor the terminal widget applies to terms; masking every "web"
/// or "db1" in sight would be noise, not privacy.
pub(crate) fn assemble_privacy_terms<'a>(
    derived: impl Iterator<Item = &'a str>,
    always_mask: &str,
    never_mask: &str,
) -> Vec<String> {
    let never: std::collections::HashSet<String> =
        parse_privacy_list(never_mask).collect();
    let mut terms: Vec<String> = derived
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|t| !never.contains(t))
        .chain(parse_privacy_list(always_mask))
        .filter(|t| t.len() >= 4)
        .collect();
    terms.sort_unstable();
    terms.dedup();
    terms
}

/// Mask exact, case-insensitive, token-bounded occurrences of `terms`
/// (lowercase) in `s`. The literal-match counterpart of the terminal's
/// KnownHost spans: plain DNS names have no detectable shape (file
/// extensions collide with ccTLDs), so the known values are matched
/// exactly instead of guessed.
fn mask_terms(s: &str, terms: &[String]) -> String {
    let lower = s.to_ascii_lowercase();
    let bytes = s.as_bytes();
    let is_tok =
        |b: u8| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-';
    // Collect bounded match ranges first; terms can overlap (one host a
    // substring of another) and ranges must merge, not double-mask.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for term in terms {
        if term.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(pos) = lower[from..].find(term.as_str()) {
            let a = from + pos;
            let b = a + term.len();
            let bounded = (a == 0 || !is_tok(bytes[a - 1]))
                && (b >= bytes.len() || !is_tok(bytes[b]));
            if bounded {
                ranges.push((a, b));
            }
            from = b;
        }
    }
    if ranges.is_empty() {
        return s.to_string();
    }
    ranges.sort_unstable();
    let mut out = String::with_capacity(s.len());
    let mut last = 0;
    for (a, b) in ranges {
        if a < last {
            continue; // swallowed by a previous (overlapping) range
        }
        out.push_str(&s[last..a]);
        out.push_str(&mask_blocks(&s[a..b]));
        last = b;
    }
    out.push_str(&s[last..]);
    out
}

/// Privacy Mode reveal toggle (the eye icon). Shows an open eye while
/// revealed (accent tint) and a struck-through eye while masked, with a
/// tooltip describing the action. Shared by the Logs view, the session-log
/// viewer header and the Known Hosts view so the reveal affordance is the
/// same everywhere. Drives `Message::TogglePrivacyReveal`.
pub(crate) fn privacy_reveal_btn<'a>(revealed: bool) -> Element<'a, Message> {
    let (glyph, tip_key) = if revealed {
        (iced_fonts::lucide::eye(), "privacy_hide")
    } else {
        (iced_fonts::lucide::eye_off(), "privacy_reveal")
    };
    let icon = glyph.size(13).color(if revealed {
        OryxisColors::t().accent
    } else {
        OryxisColors::t().text_secondary
    });
    let b = button(
        container(icon)
            .center(Length::Fixed(24.0))
            .height(Length::Fixed(24.0))
            .width(Length::Fixed(28.0)),
    )
    .on_press(Message::TogglePrivacyReveal)
    .style(move |_, status| {
        let bg = match status {
            BtnStatus::Hovered => Color::from_rgba(1.0, 1.0, 1.0, 0.08),
            BtnStatus::Pressed => Color::from_rgba(1.0, 1.0, 1.0, 0.12),
            _ if revealed => Color { a: 0.12, ..OryxisColors::t().accent },
            _ => Color::TRANSPARENT,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            border: Border {
                radius: Radius::from(6.0),
                color: OryxisColors::t().border,
                width: 1.0,
            },
            ..Default::default()
        }
    });
    iced::widget::tooltip(
        b,
        container(text(crate::i18n::t(tip_key)).size(11))
            .padding(Padding { top: 4.0, right: 8.0, bottom: 4.0, left: 8.0 })
            .style(|_| container::Style {
                background: Some(Background::Color(OryxisColors::t().bg_surface)),
                border: Border {
                    radius: Radius::from(6.0),
                    color: OryxisColors::t().border,
                    width: 1.0,
                },
                ..Default::default()
            }),
        iced::widget::tooltip::Position::Bottom,
    )
    .boxed()
}

#[cfg(test)]
mod tests {
    use super::{assemble_privacy_terms, mask_blocks, redact_for_display};
    use oryxis_terminal::PrivacyClasses;

    /// Every class on, the default runtime state.
    fn all() -> PrivacyClasses {
        PrivacyClasses::default()
    }

    #[test]
    fn terms_include_usernames_and_respect_never_mask() {
        // Saved usernames join the terms (issue #78: `ls -la` owner
        // columns); generic ones on the never list stay readable.
        let derived = ["web01.prod.internal", "koobs", "root"];
        let terms = assemble_privacy_terms(
            derived.iter().copied(),
            "",
            "root, admin, ubuntu",
        );
        assert_eq!(terms, vec!["koobs".to_string(), "web01.prod.internal".to_string()]);
    }

    #[test]
    fn always_mask_beats_never_mask_and_length_floor_holds() {
        // An explicit always-add wins over the seeded never default;
        // sub-4-char entries are dropped like the widget drops them.
        let terms = assemble_privacy_terms(
            ["bob"].iter().copied(),
            "ROOT, acme-corp, db1",
            "root",
        );
        // "bob" (derived, < 4) and "db1" (always, < 4) fall to the
        // floor; "root" survives via the always list, lowercased.
        assert_eq!(terms, vec!["acme-corp".to_string(), "root".to_string()]);
    }

    #[test]
    fn terms_dedupe_case_insensitively() {
        let terms = assemble_privacy_terms(
            ["Web01.Prod", "web01.prod"].iter().copied(),
            "web01.prod",
            "",
        );
        assert_eq!(terms, vec!["web01.prod".to_string()]);
    }

    #[test]
    fn mask_blocks_covers_separators_too() {
        assert_eq!(mask_blocks("192.168.0.4"), "███████████");
        assert_eq!(mask_blocks("deploy@web"), "██████████");
        assert_eq!(mask_blocks("a b"), "█ █");
    }

    #[test]
    fn redacts_ip_and_user_host() {
        assert_eq!(
            redact_for_display("ssh deploy@web from 192.168.0.4", &[], all()),
            format!("ssh {} from {}", mask_blocks("deploy@web"), mask_blocks("192.168.0.4"))
        );
    }

    #[test]
    fn redacts_credentials_embedded_in_a_link_target() {
        // The C3 reveal chip runs OSC 8 targets through this masker before
        // display: a URI can embed `user@host` or an IP that Privacy Mode
        // must hide just like the live terminal cells do.
        assert_eq!(
            redact_for_display("https://deploy@web.example.com/path", &[], all()),
            format!("https://{}/path", mask_blocks("deploy@web.example.com")),
        );
        assert_eq!(
            redact_for_display("http://192.168.0.4:8080/admin", &[], all()),
            format!("http://{}:8080/admin", mask_blocks("192.168.0.4")),
        );
    }

    #[test]
    fn redacts_windows_home_dir_username_only() {
        assert_eq!(
            redact_for_display(r"PS C:\Users\koobs> winget upgrade", &[], all()),
            format!(r"PS C:\Users\{}> winget upgrade", mask_blocks("koobs"))
        );
    }

    #[test]
    fn redacts_unix_home_dir_username_only() {
        assert_eq!(
            redact_for_display("cd /home/wilson/dev", &[], all()),
            format!("cd /home/{}/dev", mask_blocks("wilson"))
        );
        assert_eq!(
            redact_for_display("/Users/wilson/Library", &[], all()),
            format!("/Users/{}/Library", mask_blocks("wilson"))
        );
    }

    #[test]
    fn home_dir_marker_is_case_insensitive() {
        assert_eq!(
            redact_for_display(r"c:\users\bob>", &[], all()),
            format!(r"c:\users\{}>", mask_blocks("bob"))
        );
    }

    #[test]
    fn plain_text_is_untouched() {
        assert_eq!(
            redact_for_display("winget upgrade Name Id", &[], all()),
            "winget upgrade Name Id"
        );
    }

    #[test]
    fn redacts_ipv6_forms() {
        assert_eq!(
            redact_for_display("ping ::1 ok", &[], all()),
            format!("ping {} ok", mask_blocks("::1"))
        );
        assert_eq!(
            redact_for_display("via 2001:db8::1 dev eth0", &[], all()),
            format!("via {} dev eth0", mask_blocks("2001:db8::1"))
        );
        assert_eq!(
            redact_for_display("addr 2001:0db8:85a3:0000:0000:8a2e:0370:7334", &[], all()),
            format!("addr {}", mask_blocks("2001:0db8:85a3:0000:0000:8a2e:0370:7334"))
        );
    }

    #[test]
    fn ipv6_with_embedded_ipv4_is_fully_masked() {
        assert_eq!(
            redact_for_display("nat ::ffff:192.0.2.1 ok", &[], all()),
            format!("nat {} ok", mask_blocks("::ffff:192.0.2.1"))
        );
    }

    #[test]
    fn timestamps_and_rust_paths_are_not_ipv6() {
        assert_eq!(
            redact_for_display("12:34:56 build ok", &[], all()),
            "12:34:56 build ok"
        );
        assert_eq!(
            redact_for_display("error in std::io::Error", &[], all()),
            "error in std::io::Error"
        );
        assert_eq!(
            redact_for_display("mac aa:bb:cc:dd:ee:ff up", &[], all()),
            "mac aa:bb:cc:dd:ee:ff up"
        );
    }

    #[test]
    fn known_terms_masked_token_bounded_case_insensitive() {
        let terms = vec!["web01.prod.internal".to_string()];
        assert_eq!(
            redact_for_display("Connected to WEB01.prod.internal ok", &terms, all()),
            format!("Connected to {} ok", mask_blocks("WEB01.prod.internal"))
        );
        // Inside a longer token: no match.
        assert_eq!(
            redact_for_display("web01.prod.internal-backup", &terms, all()),
            "web01.prod.internal-backup"
        );
    }

    #[test]
    fn version_marked_quads_redact_like_any_address() {
        // The issue #53 version-marker exemption was removed 2026-07-19
        // (owner call): a range-valid quad is byte-for-byte an address
        // and hostile output could print `version <ip>` to keep a real
        // one readable. Accidentally masking a version string is the
        // accepted error, mirroring the terminal widget.
        assert_eq!(
            redact_for_display("pandoc version 3.9.0.2 installed", &[], all()),
            format!("pandoc version {} installed", mask_blocks("3.9.0.2"))
        );
        assert_eq!(
            redact_for_display("Server nginx/1.2.3.4 x", &[], all()),
            format!("Server nginx/{} x", mask_blocks("1.2.3.4"))
        );
    }

    #[test]
    fn ambiguous_quad_table_masks_in_privacy() {
        // A bare four-octet all-<=255 quad with NO marker glued to it is
        // byte-for-byte an IP; Privacy Mode masks it. A winget version
        // table (`3.9.0.2  3.13.0`) and two IPs on an `ip route` line are
        // the SAME shape, so the safe error is to mask. Versions that
        // carry a local marker (see the test above) stay readable.
        let s = "Python 3  Python.3  3.9.0.2  3.13.0  winget";
        assert_eq!(
            redact_for_display(s, &[], all()),
            format!("Python 3  Python.3  {}  3.13.0  winget", mask_blocks("3.9.0.2"))
        );
        let s2 = "Visual Studio Code  1.96.0.0  1.96.0.1";
        assert_eq!(
            redact_for_display(s2, &[], all()),
            format!(
                "Visual Studio Code  {}  {}",
                mask_blocks("1.96.0.0"),
                mask_blocks("1.96.0.1")
            )
        );
    }

    #[test]
    fn sibling_ip_not_unmasked_by_a_version_on_the_line() {
        // The leak class per-candidate scoping closes: a real public IP
        // sharing a line with a genuine version token must still mask.
        let s = "app 5.6.7 listening on 8.8.8.8";
        assert_eq!(
            redact_for_display(s, &[], all()),
            format!("app 5.6.7 listening on {}", mask_blocks("8.8.8.8"))
        );
        // Two distinct public IPs on one route line: both mask.
        let s2 = "default via 203.0.113.1 dev eth0 src 203.0.113.55";
        assert_eq!(
            redact_for_display(s2, &[], all()),
            format!(
                "default via {} dev eth0 src {}",
                mask_blocks("203.0.113.1"),
                mask_blocks("203.0.113.55")
            )
        );
    }

    #[test]
    fn version_context_is_line_scoped() {
        // The keyword on line 1 must not classify the address on line 2.
        let s = "checking for updates\nconnected to 203.0.113.7 ok";
        assert_eq!(
            redact_for_display(s, &[], all()),
            format!("checking for updates\nconnected to {} ok", mask_blocks("203.0.113.7"))
        );
    }

    #[test]
    fn range_invalid_quad_dot_not_redacted() {
        // The regex arm was shape-only; it now adopts the widget's octet
        // range check, so `999.1.1.1` stops over-masking.
        assert_eq!(redact_for_display("token 999.1.1.1 raw", &[], all()), "token 999.1.1.1 raw");
    }

    #[test]
    fn real_ips_still_redacted() {
        assert_eq!(
            redact_for_display("ping 8.8.8.8", &[], all()),
            format!("ping {}", mask_blocks("8.8.8.8"))
        );
        // Private ranges override version context.
        assert_eq!(
            redact_for_display("update available at 192.168.1.10", &[], all()),
            format!("update available at {}", mask_blocks("192.168.1.10"))
        );
    }

    #[test]
    fn vault_term_quad_dot_always_redacted() {
        let terms = vec!["3.9.0.2".to_string()];
        assert_eq!(
            redact_for_display("installed 3.9.0.2 available", &terms, all()),
            format!("installed {} available", mask_blocks("3.9.0.2"))
        );
    }

    #[test]
    fn class_gates_disable_each_shape() {
        // Usernames off: prompt tokens and home-dir names stay readable.
        let no_users = PrivacyClasses { usernames: false, ..PrivacyClasses::default() };
        assert_eq!(
            redact_for_display("ssh deploy@web in /home/bob", &[], no_users),
            "ssh deploy@web in /home/bob"
        );
        // Public IPs off: the private range still masks.
        let no_public = PrivacyClasses { public_ips: false, ..PrivacyClasses::default() };
        assert_eq!(
            redact_for_display("ping 8.8.8.8 and 192.168.0.4", &[], no_public),
            format!("ping 8.8.8.8 and {}", mask_blocks("192.168.0.4"))
        );
        // Private IPs off: public masks, loopback v6 stays readable.
        let no_private = PrivacyClasses { private_ips: false, ..PrivacyClasses::default() };
        assert_eq!(
            redact_for_display("ping 8.8.8.8 and 192.168.0.4 ::1", &[], no_private),
            format!("ping {} and 192.168.0.4 ::1", mask_blocks("8.8.8.8"))
        );
        // A vault term always masks: class filtering happens upstream in
        // privacy_terms(), so a term that reached this fn is wanted.
        let none = PrivacyClasses { public_ips: false, private_ips: false, usernames: false };
        let terms = vec!["8.8.8.8".to_string()];
        assert_eq!(
            redact_for_display("ping 8.8.8.8", &terms, none),
            format!("ping {}", mask_blocks("8.8.8.8"))
        );
    }

    #[test]
    fn usernames_off_still_masks_an_ip_after_the_at() {
        // The name stays, but the IP host part masks by its own class,
        // matching the live terminal (which scans the IP independently).
        let no_users = PrivacyClasses { usernames: false, ..PrivacyClasses::default() };
        assert_eq!(
            redact_for_display("ssh deploy@8.8.8.8 now", &[], no_users),
            format!("ssh deploy@{} now", mask_blocks("8.8.8.8"))
        );
        // A hostname after @ stays (that shape is the terms list's job).
        assert_eq!(
            redact_for_display("ssh deploy@web now", &[], no_users),
            "ssh deploy@web now"
        );
        // Public-IPs also off: the IP after @ stays readable.
        let none = PrivacyClasses { public_ips: false, private_ips: false, usernames: false };
        assert_eq!(
            redact_for_display("ssh deploy@8.8.8.8", &[], none),
            "ssh deploy@8.8.8.8"
        );
    }

    #[test]
    fn overlapping_terms_do_not_double_mask() {
        let terms = vec!["prod.internal".to_string(), "web01.prod.internal".to_string()];
        // Both terms match at overlapping positions; output masks the
        // region once and keeps total length.
        let out = redact_for_display("web01.prod.internal down", &terms, all());
        assert_eq!(out, format!("{} down", mask_blocks("web01.prod.internal")));
    }
}
