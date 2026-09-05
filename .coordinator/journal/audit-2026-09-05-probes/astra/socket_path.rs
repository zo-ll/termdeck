
use std::{os::unix::fs::{symlink,PermissionsExt},fs};
fn main(){let root=std::env::args().nth(1).unwrap();let base=std::path::Path::new(&root);let victim=base.join("unrelated-directory");fs::create_dir_all(&victim).unwrap();fs::set_permissions(&victim,fs::Permissions::from_mode(0o755)).unwrap();symlink(&victim,base.join("termdeck")).unwrap();let listener=termdeck::ctl::Listener::bind().unwrap();println!("rendezvous followed symlink={} victim_permissions={:o}",listener.path().starts_with(base),fs::metadata(&victim).unwrap().permissions().mode()&0o777);}
