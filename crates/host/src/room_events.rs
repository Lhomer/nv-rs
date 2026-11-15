//! What the active ragdoll did, as JSON lines for the test room's scripted runs (the Linux room,
//! docs/LINUX.md): each hit's reaction, the flinch, the catch steps and how they ended (a trip
//! among them), a brace and what on, a fall and why, the switch into the ragdoll. Off until the
//! room's app turns it on ([`set_on`]); then each event is kept until taken ([`take`]). Nothing of
//! the game's is changed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);
static EVENTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The events kept from now on (`true`), or no more.
pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

/// Whether events are kept.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// An event `kind` with its fields (`"name":value` pairs, already JSON, comma-separated): kept as
/// one JSON object.
pub fn record(kind: &str, fields: &str) {
    if !on() {
        return;
    }
    let line = if fields.is_empty() {
        format!("{{\"event\":\"{kind}\"}}")
    } else {
        format!("{{\"event\":\"{kind}\",{fields}}}")
    };
    EVENTS.lock().unwrap_or_else(|p| p.into_inner()).push(line);
}

/// The events kept since the last take.
pub fn take() -> Vec<String> {
    std::mem::take(&mut *EVENTS.lock().unwrap_or_else(|p| p.into_inner()))
}

/// A string as a JSON string.
pub fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A number as JSON (a value that is not finite as null).
pub fn num(v: f32) -> String {
    if v.is_finite() {
        format!("{v:.3}")
    } else {
        "null".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_json_lines() {
        set_on(true);
        record(
            "brace",
            &format!("\"on\":{},\"top\":{}", text("waist \"rail\""), num(70.0)),
        );
        record("fall", "");
        let got = take();
        assert_eq!(
            got[0],
            "{\"event\":\"brace\",\"on\":\"waist \\\"rail\\\"\",\"top\":70.000}"
        );
        assert_eq!(got[1], "{\"event\":\"fall\"}");
        assert!(take().is_empty());
        set_on(false);
        record("x", "");
        assert!(take().is_empty());
    }
}
