
use termdeck::{contracts::{TerminalId,ScreenSize,CellContent},engine::VtFrameAdapter};
fn rss()->String{std::fs::read_to_string("/proc/self/status").unwrap().lines().find(|l|l.starts_with("VmRSS:")).unwrap().into()}
fn main(){
 let mut vt=VtFrameAdapter::new(TerminalId::new("probe"),ScreenSize::new(2,2),1);
 vt.feed(b"\x1b]0;"); println!("before OSC {}",rss());
 let bytes=vec![b'x';4096]; for i in 0..4096{vt.feed(&bytes);if i==2047 || i==4095{println!("unterminated OSC {}MiB {}",(i+1)/256,rss());}}
 let mut vt=VtFrameAdapter::new(TerminalId::new("combining"),ScreenSize::new(2,2),1);vt.feed(b"a");
 let marks="\u{301}".repeat(65536);let frame=vt.feed(marks.as_bytes());
 if let CellContent::Glyph{text,..}=&frame.cells[0].content{println!("single cell retained {} scalars / {} bytes",text.chars().count(),text.len());}
}
