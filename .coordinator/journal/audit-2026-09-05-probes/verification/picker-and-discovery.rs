// #144a: match_at returns offsets into a lowercased string, sliced on the original.
// #144b: grouped and direct discovery allocate the same terminal identity.
// Expected: 'end byte index 4 is out of bounds for string of length 3', then
// discovered = ["fe-web", "fe-web"].

fn main() {
    // F6: match_at offsets computed on a lowercased string, sliced on the original.
    let name = "\u{130}x"; // İx
    let query = "x";
    println!("name bytes = {}, lowercased bytes = {}", name.len(), name.to_lowercase().len());
    match termdeck::ui::picker::match_at(name, query) {
        Some((start, end)) => {
            println!("match_at -> ({start}, {end})");
            let r = std::panic::catch_unwind(|| name[start..end].to_owned());
            println!("slice original: {r:?}");
        }
        None => println!("no match"),
    }

    // F11: grouped and direct discovery allocating the same identity.
    let root = &std::env::temp_dir().join("termdeck-144-probe");
    let root = root.as_path();
    let _ = std::fs::remove_dir_all(root);
    for p in ["frontends/web/.git", "fe-web/.git"] {
        std::fs::create_dir_all(root.join(p)).unwrap();
    }
    let ws = termdeck::cli::discover_workspace(root).unwrap();
    let names: Vec<String> = ws.projects.iter().map(|p| p.terminal.to_string()).collect();
    println!("discovered = {names:?}");
    let mut sorted = names.clone();
    sorted.sort();
    let before = sorted.len();
    sorted.dedup();
    println!("duplicate identities: {}", before != sorted.len());
}
