use std::io;
use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};

pub trait ProcessStarter<Args> {
    fn start_process(&self, args: &Args) -> io::Result<Child>;
}

pub struct Streamlink {}
impl ProcessStarter<String> for Streamlink {
    fn start_process(&self, args: &String) -> io::Result<Child> {
        Command::new("streamlink")
            .arg("-v")
            .arg(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null()) // TODO write to log file
            .stderr(Stdio::null()) // TODO write to log file
            .spawn()
    }
}

pub struct Chat {}
impl ProcessStarter<String> for Chat {
    fn start_process(&self, args: &String) -> io::Result<Child> {
        let path = format!("file:///opt/home_back/chat.html?channel={}", args);
        Command::new("firefox")
            .arg("-kiosk")
            .arg("-private-window")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null()) // TODO write to log file
            .stderr(Stdio::null()) // TODO write to log file
            .spawn()
    }
}

pub struct ProcessHandler<Args: PartialEq, T: ProcessStarter<Args> + 'static> {
    open_process: Mutex<Option<(Arc<Args>, Child)>>,
    t: T,
    on_stop: Option<fn(Arc<Args>)>,
}

impl <Args: PartialEq, T: ProcessStarter<Args>> ProcessHandler<Args, T> {

    pub fn new(t: T, on_stop: Option<fn(Arc<Args>)>) -> ProcessHandler<Args, T> {
        ProcessHandler {open_process: Mutex::from(None), t, on_stop}
    }

    pub fn running(&self) -> Option<Arc<Args>> {
        self.check_process();

        return match &*self.open_process.lock().unwrap() {
            Some((stream, _)) => Some(stream.clone()),
            None              => None
        };
    }

    pub fn start(&self, args: Args) -> io::Result<Arc<Args>> {
        //check if that stream is already running
        if let Some(s) = self.running() {
            if *s == args {
                return Ok(s);
            }
        }

        let mut open_stream = self.open_process.lock().unwrap(); 
        if let Some((args, process)) = &mut *open_stream {
            process.kill()?;
            process.wait()?;
            if let Some(callback) = self.on_stop {callback(args.clone())}
        }
        
        let process = self.t.start_process(&args)?;

        let arc = Arc::new(args);
        *open_stream = Some((arc.clone(), process));
        return Ok(arc.clone());
    }

    pub fn stop(&self) -> io::Result<()> {
        let mut open_stream = self.open_process.lock().unwrap();
        if let Some((args, process)) = &mut *open_stream {
            process.kill()?;
            process.wait()?;
            if let Some(callback) = self.on_stop {callback(args.clone())}
            *open_stream = None;
        }
        return Ok(());
    }

    fn check_process(&self) { 
        let mut open_stream = self.open_process.lock().unwrap();
        if let Some((args, process)) = &mut *open_stream {
            if process.try_wait().unwrap().is_some() {
                if let Some(callback) = self.on_stop {callback(args.clone())}
                *open_stream = None
            }
        }
    }

}
