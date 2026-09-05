
use termdeck::{contracts::*,engine::FakeEngine,ui::*};
use ratatui::{Terminal,backend::TestBackend};
fn main(){
 std::panic::set_hook(Box::new(|_|{}));
 let sheetstate=SheetState::new(&[]); let sheet=Sheet{state:&sheetstate,rows:&[],roots:&[],open:&[],home:None,next_pane:2};
 for h in [1,8,12,13,24]{println!("sheet height {h} panic={}",std::panic::catch_unwind(||sheet.rect(ratatui::layout::Rect::new(0,0,80,h))).is_err());}
 let mut state=PickerState::at("/tmp");state.begin_filter();state.push_filter('x');
 let listing=Listing::of(vec![Entry::repository("İx","/tmp/İx")]);
 let mut terminal=Terminal::new(TestBackend::new(144,42)).unwrap();
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||terminal.draw(|frame|Picker{state:&state,listing:&listing,roots:&[],home:None}.render(frame)).map(|_|())));
 println!("picker name İx query x panic={}",result.is_err());
 let mut checked=0; let mut failures=Vec::new();
 for n in [1,2,5,16]{
 let projects:Vec<_>=(0..n).map(|i|Project{terminal:TerminalId::new(format!("p{i}")),path:"/tmp".into(),command:vec!["bash".into()],shell_hook:false}).collect();
 let mut engine=FakeEngine::new(projects.iter().map(|p|p.terminal.clone()));
 for p in &projects{engine.set_status(&p.terminal,TerminalStatus::Exited{code:Some(1)});}
 for mode in 0..5{
 let mut state=DeckState::new(n); let now=Timestamp::default();
 match mode{1=>{state.toggle_collapse_all();},2=>{state.apply(&ActionCommand::ToggleZoom,&projects,now);},3=>{state.apply(&ActionCommand::ShowHelp,&projects,now);},4=>{state.apply(&ActionCommand::ToggleScrollback,&projects,now);},_=>{}}
 for w in [1,5,6,7,23,24,52,80,99,100,119,120,144]{for h in [1,2,3,4,5,6,8,12,13,24,42]{
 let mut terminal=Terminal::new(TestBackend::new(w,h)).unwrap(); let notifies=Notifications::new();
 let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||terminal.draw(|frame|Deck{workspace:"audit",projects:&projects,state:&state,notifies:&notifies,master_ratio:state.master_ratio(),now}.render(&engine,frame)).map(|_|()))); checked+=1;
 if result.is_err(){failures.push((n,mode,w,h));}
 }}
 }}
 println!("deck render boundary sweep: {checked} cases, {} panics, first {:?}",failures.len(),&failures[..failures.len().min(10)]);
}
