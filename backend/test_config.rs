use std::fs;

fn main() {
    let content = fs::read_to_string("config/factory.json").expect("Failed to read file");
    println!("File content length: {}", content.len());
    println!("First 100 chars: {}", &content[..100.min(content.len())]);
}
