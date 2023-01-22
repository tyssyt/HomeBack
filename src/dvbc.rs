use std::env;
use std::fs;
use log::info;
use std::time::{SystemTime, Duration, Instant};
use reqwest::blocking::Client;
use std::sync::{Arc, Mutex};
use super::files::sanitize_path;
use std::process::Command;
use serde::Serialize;
use threadpool::ThreadPool;
use std::error::Error;

// TODO switch to non-blocking reqwest

lazy_static! {
    static ref WEB_BASE_FOLDER : String = env::var("WEB_BASE_FOLDER").expect("WEB_BASE_FOLDER not set");
}

pub struct DvbC {
    client: Client,
    url_hd: String,
    url_sd: String,
    url_radio: String,
    channels: Mutex<Option<Arc<Channels>>>,
}

pub struct Channels {
    pub tv:    Vec<Channel>,
    pub radio: Vec<Channel>,
    fetched_at: Instant
}

#[derive(Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub url: String,
}

fn needs_update(channels: &Option<Arc<Channels>>) -> bool {
    channels.is_none() || Instant::now().duration_since(channels.as_ref().unwrap().fetched_at).as_secs() > 60*60
}

impl DvbC {

    pub fn new() -> DvbC {
        let router_url = env::var("ROUTER_URL").expect("ROUTER_URL not set");
        return DvbC {
            client:    Client::builder().timeout(Duration::from_secs(2)).build().unwrap(),
            url_hd:    format!("{}{}", router_url, "/dvb/m3u/tvhd.m3u"),
            url_sd:    format!("{}{}", router_url, "/dvb/m3u/tvsd.m3u"),
            url_radio: format!("{}{}", router_url, "/dvb/m3u/radio.m3u"),
            channels:  Mutex::new(None),
        };
    }

    pub fn get_channels(&self) -> Option<Arc<Channels>> {
        let mut lock = self.channels.lock().unwrap();
        if needs_update(&*lock) {
            *lock = self.fetch_all_channels().ok().map(|c| Arc::new(c));
        }
        return lock.clone();
    }

    fn fetch_all_channels(&self) -> Result<Channels, reqwest::Error> {
        let mut tv =   self.fetch_category(&self.url_hd)?;
        tv.append(&mut self.fetch_category(&self.url_sd)?);
        let radio  =   self.fetch_category(&self.url_radio)?;
        info!("Loaded DvbC: {} TV & {} Radio Channels", tv.len(), radio.len());
        Ok(Channels {
            tv,
            radio: radio,
            fetched_at: Instant::now()
        })
    }

    fn fetch_category(&self, url: &str) -> Result<Vec<Channel>, reqwest::Error> {
        let text = self.client.get(url).send()?.text()?;
        let mut lines = text.lines().skip(1);
        
        let mut channels = Vec::new();
        loop {
            if let (Some(first), Some(_second), Some(third)) = (lines.next(), lines.next(), lines.next()) {
                channels.push(Channel {name: String::from(&first[10..]), url: String::from(third)})
            } else {
                break;
            }
        }
        Ok(channels)
    }
}

pub struct DvbCPreviews {
    threadpool: Mutex<ThreadPool>, //Mutex because: https://github.com/rust-threadpool/rust-threadpool/issues/96
}

#[derive(Serialize)]
pub struct ChannelPreview {
    url: String,
    created: Option<u128>,
}

impl DvbCPreviews {

    pub fn new() -> DvbCPreviews {
        DvbCPreviews {
            threadpool: Mutex::new(ThreadPool::new(2))
        }
    }

    pub fn get_preview(&'static self, channel: &Channel) -> ChannelPreview {
        // TODO this is not as efficient as it could be w.r.t. handling and copying strings
        let url = sanitize_path(&format!("/img/tv/preview/{}.jpg", &channel.name.replace(" ", "_"))).into_os_string().into_string().unwrap();
        let path = format!("{}{}", &*WEB_BASE_FOLDER, &url);

        let created = fs::metadata(&path).ok().map(|meta| meta.created().ok()).flatten();
        if created.map(|ts| ts.elapsed().ok()).flatten().map_or(false, |duration| duration.as_secs() <= 60*5) {
            return ChannelPreview{url, created: created.map(|ts| ts.duration_since(SystemTime::UNIX_EPOCH).ok()).flatten().map(|duration| duration.as_millis())};
        }

        // there is no file or it is too old, add to queue for creation
        self.request_preview(channel).unwrap(); // TODO do not unwrap
        ChannelPreview{url, created: None}
    }

    fn request_preview(&'static self, channel: &Channel) -> Result<(), Box<dyn Error>> { // TODO actual error Type, no need to box
        let lock = self.threadpool.lock()?;
        if lock.queued_count() < 2 {
            let clone = channel.clone();
            lock.execute(move || self.create_preview(clone));
        }
        Ok(())
    }

    fn create_preview(&self, channel: Channel) {
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
            .status().unwrap(); // TODO deal with the unwrap!
    }
}