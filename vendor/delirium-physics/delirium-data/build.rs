// Added by nv-rs (vendor/README.md): kv.rs's maps are kept as hex text (src/*.map.hex, one
// published byte each two digits, lines starting with # are notes) so that the repository holds
// no binary file; this writes the published bytes back into OUT_DIR for kv.rs's include_bytes!.
use std::{env, fs, path::Path};

fn main() {
    let out = env::var("OUT_DIR").expect("OUT_DIR");
    for name in ["kv.map", "kv-disc.map", "kv-pc1.map", "kv-pc2.map", "kv-rva.map"] {
        let src = format!("src/{name}.hex");
        println!("cargo:rerun-if-changed={src}");
        let text = fs::read_to_string(&src).unwrap_or_else(|e| panic!("{src}: {e}"));
        let digits: String = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .flat_map(|l| l.trim().chars())
            .collect();
        assert!(digits.len() % 2 == 0, "{src}: an odd number of hex digits");
        let bytes: Vec<u8> = (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap_or_else(|e| panic!("{src}: {e}")))
            .collect();
        fs::write(Path::new(&out).join(name), bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}
