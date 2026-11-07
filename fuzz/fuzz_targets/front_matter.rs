//! `split_front_matter` never panics; an `Ok` split is consistent with the source, and the
//! YAML is capped at 64 KiB.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rubrica_render::split_front_matter;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok((_, body, offset)) = split_front_matter(s) {
        assert!(s.ends_with(body), "the body is not a suffix of the source");
        let head = s[..s.len() - body.len()].trim_end_matches('\n');
        let first = s.find('\n').map_or(s.len(), |i| i + 1);
        let last = head.rfind('\n').map_or(0, |i| i + 1);
        let yaml = head
            .len()
            .saturating_sub(first)
            .saturating_sub(head.len() - last);
        assert!(yaml <= 64 * 1024, "{yaml} bytes of YAML accepted");
        assert!(
            offset <= s.lines().count(),
            "offset {offset} past the last line"
        );
    }
});
