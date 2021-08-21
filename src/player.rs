use std::io;
use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};


pub struct Player {
    open_stream: Mutex<Option<(Arc<String>, Child)>>,
}

impl Player {

    pub fn new() -> Player {
        Player {open_stream: Mutex::from(None)}
    }

    pub fn stream(&self) -> Option<Arc<String>> {
        self.check_process();

        return match &*self.open_stream.lock().unwrap() {
            Some((stream, _)) => Some(stream.clone()),
            None              => None
        };
    }

    pub fn start(&self, stream: String) -> io::Result<Arc<String>> {
        //check if that stream is already running
        if let Some(s) = self.stream() {
            if *s == stream {
                return Ok(s);
            }
        }

        let mut open_stream = self.open_stream.lock().unwrap(); 
        if let Some((_, process)) = &mut *open_stream {
            process.kill()?;
            process.wait()?;
        }
        
        let process = Command::new("streamlink")
                .arg("-v")
                .arg(&stream)
                .stdin(Stdio::null())
                .stdout(Stdio::null()) // TODO write to log file
                .stderr(Stdio::null()) // TODO write to log file
                .spawn()?;

        let arc = Arc::new(stream);
        *open_stream = Some((arc.clone(), process));
        return Ok(arc.clone());
    }

    pub fn stop(&self) -> io::Result<()> {
        let mut open_stream = self.open_stream.lock().unwrap();
        if let Some((_, process)) = &mut *open_stream {
            process.kill()?;
            process.wait()?;
            *open_stream = None;
        }
        return Ok(());
    }

    fn check_process(&self) { 
        let mut open_stream = self.open_stream.lock().unwrap();
        if let Some((_, process)) = &mut *open_stream {
            if process.try_wait().unwrap().is_some() {
                *open_stream = None
            }
        }
    }

}
