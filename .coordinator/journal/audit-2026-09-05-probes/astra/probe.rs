
use termdeck::{contracts::{Project,TerminalId,ScreenSize,TerminalEngine,EngineCommand},engine::NativeEngine};
use std::{fs,time::{Duration,Instant},thread,process::Command};
fn main(){
 let root=std::env::args().nth(1).unwrap();
 let pidfile=format!("{root}/escaped.pid");
 let code=format!("import os,time\npid=os.fork()\nif pid==0:\n os.setsid()\n open({pidfile:?},'w').write(str(os.getpid()))\n time.sleep(30)\nelse:\n time.sleep(30)\n");
 let project=Project{terminal:TerminalId::new("probe"),path:root.into(),command:vec!["python3".into(),"-c".into(),code],shell_hook:false};
 let mut engine=NativeEngine::spawn(&[project],ScreenSize::new(80,24)).unwrap();
 let deadline=Instant::now()+Duration::from_secs(3);
 while !std::path::Path::new(&pidfile).exists() && Instant::now()<deadline {thread::sleep(Duration::from_millis(10));}
 let pid=fs::read_to_string(&pidfile).unwrap();
 let start=Instant::now();engine.dispatch(EngineCommand::Shutdown);
 let stat=fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
 println!("shutdown_ms={} escaped_child_alive={} proc_stat={}", start.elapsed().as_millis(),!stat.is_empty() && !stat.contains(") Z "),stat);
 Command::new("kill").args(["-KILL",&pid]).status().unwrap();
 drop(engine);
}
