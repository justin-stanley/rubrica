//! `parse_with_offsets`: every range is in bounds and on char boundaries, and the `Start`
//! events' starts never go backwards in document order.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rubrica_markdown::{parse_with_offsets, Event, GFM};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let mut last_start = 0;
    for (e, r) in parse_with_offsets(s, GFM) {
        assert!(r.start <= r.end, "reversed range {r:?} on {e:?}");
        assert!(r.end <= s.len(), "range {r:?} past {} on {e:?}", s.len());
        assert!(s.is_char_boundary(r.start), "start of {r:?} on {e:?}");
        assert!(s.is_char_boundary(r.end), "end of {r:?} on {e:?}");
        if matches!(e, Event::Start(_)) {
            assert!(
                r.start >= last_start,
                "Start range {r:?} begins before the previous Start's {last_start}: {e:?}"
            );
            last_start = r.start;
        }
    }
});
