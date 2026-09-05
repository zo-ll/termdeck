
use termdeck::{cli::discover_workspace,engine::NativeEngine,contracts::ScreenSize};
fn main(){let root=std::env::args().nth(1).unwrap();let root=std::path::Path::new(&root).join("collision");for path in ["frontends/web/.git","fe-web/.git"]{std::fs::create_dir_all(root.join(path)).unwrap();}let w=discover_workspace(root).unwrap();println!("discovered identities={:?}",w.projects.iter().map(|p|p.terminal.to_string()).collect::<Vec<_>>());println!("spawn error={:?}",NativeEngine::spawn(&w.projects,ScreenSize::new(80,24)).err());}
