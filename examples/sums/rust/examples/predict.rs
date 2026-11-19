//! Prints the recreation's answers to a sequence file as TOML, so its
//! predictions can be committed before the reference is recorded.
//!     cargo run --example predict -- <sequences.toml>

use sums_core::{Calculator, Key};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: predict <sequences.toml>");
    let text = std::fs::read_to_string(&path).expect("read the sequence file");
    let file: toml::Table = toml::from_str(&text).expect("parse the sequence file");
    println!(
        "# The recreation's predictions for {path}, made before the reference was recorded.\n"
    );
    for seq in file["sequence"].as_array().expect("[[sequence]]") {
        let mut calc = Calculator::new();
        calc.press(Key::Clear);
        calc.press(Key::Clear);
        for key in seq["keys"].as_array().expect("keys") {
            let id = key.as_str().expect("key id");
            calc.press(Key::from_id(id).unwrap_or_else(|| panic!("unknown key {id}")));
        }
        println!("[[result]]");
        println!(
            "name = {}",
            toml::Value::from(seq["name"].as_str().unwrap())
        );
        println!("display = {}", toml::Value::from(calc.display()));
        if let Some(e) = calc.expression() {
            println!("expression = {}", toml::Value::from(e));
        }
        println!();
    }
}
