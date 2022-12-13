use std::io;
use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};
use std::str;
use log::info;
use log::error;

use super::dvbc::Channel;

pub trait ProcessStarter<Args> {
    fn start_process(&self, args: &Args) -> io::Result<Child>;
    fn on_stop(&self, _args: &Args, _process: &Child) {}
}

fn kill_mpv(parent_process_id: u32) {
    // find the process id by calling ps
    let output = match Command::new("ps")
        .arg("h")                      // don't show a header
        .arg("-o").arg("pid,ppid")     // only show the process id and the parent process id
        .arg("-C").arg("mpv").output() // only show processes with the command mpv
    {
        Ok(output) => output,
        Err(error) => {error!("could not find pid of mpv: {}", error); return},
    };

    let output_as_str = str::from_utf8(&output.stdout).expect("Output of 'ps' is not UTF-8!");

    // every line of the output is a mpv process, find the one with the right parent process id
    for line in output_as_str.lines() {
        // parse the words on the line as u32, as we expect them to be process ids
        let words: Vec<u32> = line.trim()
            .split_whitespace()
            .filter_map(|word| word.parse::<u32>().ok())
            .collect();

        if let [pid, ppid] = words[0..2] {
            if ppid == parent_process_id {                        
                if let Ok(status) = Command::new("kill").arg(pid.to_string()).status() {
                    info!("killed {} with status: {}", pid, status);
                } else {
                    error!("kill of {} failed", pid);
                }
            }
        } else {
            error!("could not parse output of ps: {}", line);
        }
    }
}

pub struct Chat {}
impl ProcessStarter<String> for Chat {
    fn start_process(&self, args: &String) -> io::Result<Child> {
        info!("opening chat: {}", &args);
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

#[derive(PartialEq)]
pub enum VideoPlayerArgs {
    Twitch(String),
    DvbC(Channel),
}

pub struct VideoPlayer{}
impl ProcessStarter<VideoPlayerArgs> for VideoPlayer {

    fn start_process(&self, args: &VideoPlayerArgs) -> io::Result<Child> {
        return match args {
            VideoPlayerArgs::Twitch(stream) => {                
                info!("opening Twitch Stream: {}", &stream);
                Command::new("streamlink")
                    //.arg("-v")
                    .arg("--player-passthrough").arg("hls,http")
                    .arg(stream)
                    .stdin(Stdio::null())
                    .spawn()
            },
            VideoPlayerArgs::DvbC(channel) => {
                info!("opening DvbC Channel: {}", &channel.name);
                Command::new("ffplay")
                    .arg("-sn")
                    .arg(&channel.url)
                    .stdin(Stdio::null())
                    .spawn()
            },
        };
    }

    fn on_stop(&self, args: &VideoPlayerArgs, process: &Child) {
        if let VideoPlayerArgs::Twitch(_) = args {
            kill_mpv(process.id());
        }
    }
    
}

pub struct ProcessHandler<Args: PartialEq, T: ProcessStarter<Args> + 'static> {
    open_process: Mutex<Option<(Arc<Args>, Child)>>,
    t: T,
    on_stop: Option<fn(&Args, &Child)>,
}

impl <Args: PartialEq, T: ProcessStarter<Args>> ProcessHandler<Args, T> {

    pub fn new(t: T, on_stop: Option<fn(&Args, &Child)>) -> ProcessHandler<Args, T> {
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
        self.stop_impl(&mut *open_stream)?;
        
        let process = self.t.start_process(&args)?;

        let arc = Arc::new(args);
        *open_stream = Some((arc.clone(), process));
        return Ok(arc.clone());
    }

    pub fn stop(&self) -> io::Result<()> {
        let mut open_stream = self.open_process.lock().unwrap();
        self.stop_impl(&mut *open_stream)?;
        *open_stream = None;
        return Ok(());
    }

    fn stop_impl(&self, open_stream: &mut Option<(Arc<Args>, Child)>) -> io::Result<()> {
        if let Some((args, process)) = open_stream {
            self.handle_callbacks(&args, &process);
            process.kill()?;
            process.wait()?;
        }
        return Ok(());
    }

    fn handle_callbacks(&self, args: &Args, process: &Child) {       // TODO check if mut  
        self.t.on_stop(args, process);
        if let Some(callback) = self.on_stop {
            callback(args, process);
        }
    }

    fn check_process(&self) { 
        let mut open_stream = self.open_process.lock().unwrap();
        if let Some((args, process)) = &mut *open_stream {
            if process.try_wait().unwrap().is_some() {
                self.handle_callbacks(args, process);
                *open_stream = None
            }
        }
    }

}
