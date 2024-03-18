use super::files::sanitize_path;
use super::dvbc::Channel;

use core::fmt;
use std::collections::VecDeque;
use std::env;
use std::fs;
use std::io;
use std::process::Child;
use std::time::SystemTimeError;
use std::time::{SystemTime, Duration, Instant};
use std::sync::{Arc, Mutex};
use std::process::Command;
use std::error::Error;
use actix_web::rt::spawn;
use actix_web::rt::task::JoinHandle;
use actix_web::rt::time::interval;
use itertools::Itertools;
use log::error;
use log::info;
use serde::Serialize;

lazy_static! {
    static ref WEB_BASE_FOLDER : String = env::var("WEB_BASE_FOLDER").expect("WEB_BASE_FOLDER not set");
}

pub struct DvbCPreviews {
    waiting: Arc<Mutex<VecDeque<Channel>>>,
    scheduler: Mutex<JoinHandle<()>>,
}

#[derive(Serialize)]
pub struct ChannelPreview {
    url: String,
    created: Option<u128>,
}

enum FileState {
    New(u128),
    Old,
    Absent,
}

impl DvbCPreviews {

    pub fn new() -> Self {
        Self::clear_preview_dir().unwrap();

        Self {
            waiting: Arc::new(Mutex::new(VecDeque::with_capacity(7))),
            scheduler: Mutex::new(spawn(async {})),
        }        
    }

    fn clear_preview_dir() -> Result<(), io::Error> {
        let path = sanitize_path(&format!("{}/img/tv/preview", &*WEB_BASE_FOLDER)).into_os_string().into_string().unwrap();
        fs::create_dir_all(&path)?;
        fs::remove_dir_all(&path)?;
        fs::create_dir(&path)
    }

    // TODO on startup, clear the entire folder

    pub fn get_preview(&self, channel: &Channel) -> Result<ChannelPreview, PreviewError> {
        // TODO this is not as efficient as it could be w.r.t. handling and copying strings
        let url = sanitize_path(&format!("/img/tv/preview/{}.jpg", &channel.name.replace(" ", "_"))).into_os_string().into_string().unwrap();
        let path = format!("{}{}", &*WEB_BASE_FOLDER, &url);

        let file_exists = match Self::get_preview_from_disk(&path)? {
            FileState::New(created) => return Ok(ChannelPreview{url, created: Some(created)}),
            FileState::Old => true,
            FileState::Absent => false,
        };

        self.request_preview(channel, file_exists);
        Ok(ChannelPreview{url, created: None})
    }

    fn get_preview_from_disk(path: &str) -> Result<FileState, PreviewError> {
        let created = match fs::metadata(&path) {
            Ok(metadata) => metadata.created()?,
            Err(_) => return Ok(FileState::Absent)
        };

        if created.elapsed().unwrap().as_secs() <= 60*5 {
            Ok(FileState::New(created.duration_since(SystemTime::UNIX_EPOCH)?.as_millis()))
        } else {
            Ok(FileState::Old)
        }
    }

    fn request_preview(&self, channel: &Channel, file_exists: bool) {
        {
            let mut waiting = self.waiting.lock().unwrap();
            if ( waiting.len() <= 5 || (!file_exists && waiting.len() <= 10) ) &&
                waiting.iter().find(|wait| wait.name == channel.name).is_none()
            {
                waiting.push_front(channel.clone());
            }
        }
        self.how_is_the_scheduler_doing();
    }

    // asking the important questions
    fn how_is_the_scheduler_doing(&self) {
        let mut scheduler = self.scheduler.lock().unwrap();
        if scheduler.is_finished() {
            *scheduler = spawn(DvbcScheduler::start(self.waiting.clone()));
        }
    }
}

struct DvbcScheduler {
    running: [Option<(Child, Channel, Instant)>; 1],
    waiting: Arc<Mutex<VecDeque<Channel>>>,
}

impl DvbcScheduler {

    async fn start(waiting: Arc<Mutex<VecDeque<Channel>>>) {
        info!("starting DvbC Preview Sceduler");

        let mut scheduler = DvbcScheduler{ running: [None], waiting };        
        let mut interval = interval(Duration::from_secs(1));
        while scheduler.schedule() {
            interval.tick().await;
        }

        info!("stopping DvbC Preview Sceduler");
    }

    fn schedule(&mut self) -> bool {
        // collect names
        let running_channels = self.running.iter()
            .flat_map(|run| run.iter())
            .map(|(_, channel, _)| channel.name.clone())
            .collect_vec();

        // for each in running, if child is done replace with None
        for i in 0..self.running.len() {
            if let Some((child, channel, instant)) = &mut self.running[i] {
               
                match child.try_wait() {
                    Ok(Some(status)) => {
                        info!("ffmpeg for {} finished with status {} in {}s", channel.name, status, instant.elapsed().as_secs());
                        self.running[i] = None;
                    },
                    Ok(None) => {},
                    Err(err) => {
                        error!("Error getting status of ffmpeg process for {}: {}", channel.name, err);
                        self.running[i] = None;
                    },
                }
            }
        }

        // count empty slots
        let empty_slots = self.running.iter().filter(|run| run.is_none()).count();
        if empty_slots == 0 {
            let waiting = self.waiting.lock().unwrap();
            return !waiting.is_empty();
        }

        // remove names from waiting and pop from queue
        let mut to_run = {
            let mut waiting = self.waiting.lock().unwrap();
            waiting.retain(|channel| !running_channels.iter().any(|name| channel.name == *name));
            let waiting_len = waiting.len(); // TODO why do I need this var? sometimes rust confuses me
            waiting.split_off(waiting_len - empty_slots)
        };
        
        // start preview creation
        for i in 0..self.running.len() {
            if to_run.is_empty() {
                break;
            }
            if self.running[i].is_none() {
                let channel = to_run.pop_back().unwrap();
                match self.create_preview(&channel) {
                    Ok(child) => self.running[i] = Some(( child, channel, Instant::now() )),
                    Err(err) => error!("Error creating ffmpeg child process: {}", err),
                }
            }
        }

        if !to_run.is_empty() {
            panic!("there were less open slots then channels removed from waiting. This should never happen!")
        }

        true
    }

    fn create_preview(&self, channel: &Channel) -> Result<Child, io::Error> {
        let path = sanitize_path(&format!("{}/img/tv/preview/{}.jpg", &*WEB_BASE_FOLDER, &channel.name.replace(" ", "_"))).into_os_string().into_string().unwrap();
        info!("calling ffmpeg to: {:?}", path);
        Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-loglevel").arg("panic")
            .arg("-y")
            .arg("-i").arg(&channel.url)
            .arg("-vframes").arg("1")
            .arg(&path)
            //.stdin(Stdio::null())
            //.stdout(Stdio::null())
            //.stderr(Stdio::null())
            .spawn()
    }
}

// TODO try to write a macro for this (or find one)
// TODO or consider just having one big enum error for all of HomeBack
pub enum PreviewError {
    IO(io::Error),
    SystemTime(SystemTimeError),
}

impl From<io::Error> for PreviewError {
    fn from(error: io::Error) -> Self {
        Self::IO(error)
    }
}
impl From<SystemTimeError> for PreviewError {
    fn from(error: SystemTimeError) -> Self {
        Self::SystemTime(error)
    }
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IO(error) => fmt::Display::fmt(error, f),
            Self::SystemTime(error) => fmt::Display::fmt(error, f),
        }
    }
}
impl fmt::Debug for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::IO(error) => fmt::Debug::fmt(error, f),
            Self::SystemTime(error) => fmt::Debug::fmt(error, f),
        }
    }
}

impl Error for PreviewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {        
        match self {
            Self::IO(error) => error.source(),
            Self::SystemTime(error) => error.source(),
        }
    }
}